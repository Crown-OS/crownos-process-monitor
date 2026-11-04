//! Runs the service on a private `dbus-daemon` (skipped when it is not
//! installed) with the allow-list backend, as the current user.

use std::error::Error;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use monitor_helper::auth::StaticAuthorizer;
use monitor_helper::{
    BUS_NAME, HelperAvailability, HelperClient, HelperError, HelperState, INTERFACE_NAME, Method,
    OBJECT_PATH, ProcessUsage, SocketOwner, serve,
};

type TestResult = Result<(), Box<dyn Error>>;

const DAEMON: &str = "dbus-daemon";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

struct PrivateBus {
    daemon: Child,
    address: String,
}

impl PrivateBus {
    fn start(name: &str) -> Result<Option<Self>, Box<dyn Error>> {
        let Some(daemon) = find_in_path(DAEMON) else {
            eprintln!("{DAEMON} is not installed; skipping");
            return Ok(None);
        };
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let socket = directory.join("bus");
        let _ = std::fs::remove_file(&socket);
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>"#,
                socket.display()
            ),
        )?;
        let child = Command::new(daemon)
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .stdout(Stdio::null())
            .spawn()?;
        let bus = Self {
            daemon: child,
            address: format!("unix:path={}", socket.display()),
        };
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while !socket.exists() {
            if Instant::now() > deadline {
                return Err("dbus-daemon did not start".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(Some(bus))
    }

    async fn connect(&self) -> zbus::Result<zbus::Connection> {
        zbus::connection::Builder::address(self.address.as_str())
            .build()
            .await
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|directory| Path::new(directory).join(program))
        .find(|candidate| candidate.is_file())
}

fn current_uid() -> Result<u32, std::io::Error> {
    std::fs::metadata("/proc/self").map(|metadata| metadata.uid())
}

/// Reads for the current user only; energy, fans and signals stay denied.
async fn start_service(bus: &PrivateBus) -> Result<zbus::Connection, Box<dyn Error>> {
    let policy = format!(
        "read-all-processes = uid:{uid}\nread-gpu-counters = uid:{uid}",
        uid = current_uid()?
    );
    let state = Arc::new(HelperState::new(StaticAuthorizer::parse(&policy)?)?);
    let connection = bus.connect().await?;
    serve(&connection, state).await?;
    Ok(connection)
}

async fn helper_proxy(connection: &zbus::Connection) -> zbus::Result<zbus::Proxy<'static>> {
    zbus::Proxy::new(connection, BUS_NAME, OBJECT_PATH, INTERFACE_NAME).await
}

#[tokio::test(flavor = "current_thread")]
async fn serves_authorized_reads_and_refuses_the_rest() -> TestResult {
    let Some(bus) = PrivateBus::start("serves")? else {
        return Ok(());
    };
    let _service = start_service(&bus).await?;
    let client = bus.connect().await?;
    let proxy = helper_proxy(&client).await?;
    let own = std::process::id();

    let usage: Vec<ProcessUsage> = proxy.call("ProcessUsage", &(vec![own],)).await?;
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].pid, own);
    assert!(usage[0].start_ticks > 0);
    assert!(usage[0].pss > 0, "a single process is measured at once");

    let everyone: Vec<ProcessUsage> = proxy.call("ProcessUsage", &(Vec::<u32>::new(),)).await?;
    assert!(everyone.iter().any(|usage| usage.pid == own));
    assert!(everyone.iter().any(|usage| usage.pid == 1));

    let owners: Vec<SocketOwner> = proxy.call("SocketOwners", &()).await?;
    assert!(
        owners.iter().any(|owner| owner.pid == own),
        "the bus sockets of this test belong to it"
    );

    for (method, body) in [("Energy", ()), ("RestoreFans", ())] {
        let denied = proxy.call::<_, _, ()>(method, &body).await;
        let error = denied.err().map(HelperError::from);
        assert!(
            matches!(error, Some(HelperError::NotAuthorized(_))),
            "{method}: {error:?}"
        );
    }
    let signal = proxy
        .call::<_, _, ()>("Signal", &(own, 1_u64, "term"))
        .await
        .err()
        .map(HelperError::from);
    assert!(
        matches!(signal, Some(HelperError::NotAuthorized(_))),
        "{signal:?}"
    );

    let version: u32 = proxy.get_property("Version").await?;
    assert_eq!(version, monitor_helper::API_VERSION);
    let capabilities: Vec<String> = proxy.get_property("Capabilities").await?;
    assert!(capabilities.iter().any(|name| name == "read-all-processes"));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn introspection_lists_exactly_the_mapped_methods() -> TestResult {
    let Some(bus) = PrivateBus::start("introspects")? else {
        return Ok(());
    };
    let _service = start_service(&bus).await?;
    let client = bus.connect().await?;
    let xml = zbus::fdo::IntrospectableProxy::builder(&client)
        .destination(BUS_NAME)
        .path(OBJECT_PATH)
        .build()
        .await?
        .introspect()
        .await?;
    let start = xml
        .find(&format!(r#"<interface name="{INTERFACE_NAME}">"#))
        .ok_or("interface missing")?;
    let interface = &xml[start..];
    let interface = &interface[..interface.find("</interface>").ok_or("unterminated")?];
    let mut exported: Vec<&str> = interface
        .split(r#"<method name=""#)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let mut mapped: Vec<&str> = Method::ALL.iter().map(|method| method.name()).collect();
    exported.sort_unstable();
    mapped.sort_unstable();
    assert_eq!(exported, mapped, "every D-Bus method has a capability");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn client_publishes_results_without_blocking() -> TestResult {
    let Some(bus) = PrivateBus::start("client")? else {
        return Ok(());
    };
    let _service = start_service(&bus).await?;
    let client = HelperClient::connect_to_address(bus.address.as_str()).ok_or("no client")?;
    let own = std::process::id();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let usage = loop {
        client.request_process_usage(vec![own]);
        if let Some(usage) = client
            .latest_process_usage()
            .filter(|usage| !usage.is_empty())
        {
            break usage;
        }
        if Instant::now() > deadline {
            return Err("no process usage arrived".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(usage[0].pid, own);
    assert_eq!(client.availability(), HelperAvailability::Available);

    let refused = client.restore_fans().await?;
    assert!(
        matches!(refused, Err(HelperError::NotAuthorized(_))),
        "{refused:?}"
    );
    assert_eq!(
        client.availability(),
        HelperAvailability::Available,
        "a refused write leaves reads available"
    );
    assert!(client.supports(monitor_helper::Capability::ReadAllProcesses));

    client.request_energy();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while client.availability() != HelperAvailability::Denied {
        if Instant::now() > deadline {
            return Err("the denied read did not mark the helper denied".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        client.capability_availability(monitor_helper::Capability::ReadEnergy),
        HelperAvailability::Denied
    );
    assert_eq!(
        client.capability_availability(monitor_helper::Capability::ReadAllProcesses),
        HelperAvailability::Available,
        "a denied energy read leaves process reads available"
    );
    Ok(())
}
