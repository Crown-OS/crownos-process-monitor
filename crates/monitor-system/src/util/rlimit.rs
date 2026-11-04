use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

/// One persistent `stat` descriptor per process needs more than the usual
/// soft limit of 1024 descriptors.
pub(crate) fn raise_open_file_limit() {
    let limit = getrlimit(Resource::Nofile);
    if limit.current != limit.maximum {
        let raised = Rlimit {
            current: limit.maximum,
            maximum: limit.maximum,
        };
        let _ = setrlimit(Resource::Nofile, raised);
    }
}
