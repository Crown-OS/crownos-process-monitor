// SPDX-License-Identifier: GPL-2.0-or-later
//
// Per-cgroup byte counters for every protocol. Attached to the cgroup v2
// root with BPF_F_ALLOW_MULTI, so they see the packets of every socket in
// every descendant cgroup. Built by build.rs with
// `clang -O2 -target bpf`; self-contained, no libbpf headers needed.

typedef unsigned int u32;
typedef unsigned long long u64;

#define SEC(name) __attribute__((section(name), used))
#define ALWAYS_INLINE static inline __attribute__((always_inline))

#define BPF_MAP_TYPE_LRU_PERCPU_HASH 10
#define BPF_NOEXIST 1
#define MAX_CGROUPS 4096
#define LET_PACKET_PASS 1

struct __sk_buff {
	u32 len;
};

struct bpf_map_def {
	u32 type;
	u32 key_size;
	u32 value_size;
	u32 max_entries;
	u32 map_flags;
};

struct traffic {
	u64 received;
	u64 transmitted;
};

static void *(*bpf_map_lookup_elem)(void *map, const void *key) = (void *)1;
static long (*bpf_map_update_elem)(void *map, const void *key, const void *value, u64 flags) = (void *)2;
static u64 (*bpf_skb_cgroup_id)(struct __sk_buff *skb) = (void *)79;

struct bpf_map_def SEC("maps") CGROUP_TRAFFIC = {
	.type = BPF_MAP_TYPE_LRU_PERCPU_HASH,
	.key_size = sizeof(u64),
	.value_size = sizeof(struct traffic),
	.max_entries = MAX_CGROUPS,
};

ALWAYS_INLINE struct traffic *traffic_of(struct __sk_buff *skb)
{
	u64 cgroup = bpf_skb_cgroup_id(skb);
	struct traffic *counters = bpf_map_lookup_elem(&CGROUP_TRAFFIC, &cgroup);
	if (counters)
		return counters;
	struct traffic zero = { 0, 0 };
	bpf_map_update_elem(&CGROUP_TRAFFIC, &cgroup, &zero, BPF_NOEXIST);
	return bpf_map_lookup_elem(&CGROUP_TRAFFIC, &cgroup);
}

SEC("cgroup_skb/ingress")
int count_ingress(struct __sk_buff *skb)
{
	struct traffic *counters = traffic_of(skb);
	if (counters)
		counters->received += skb->len;
	return LET_PACKET_PASS;
}

SEC("cgroup_skb/egress")
int count_egress(struct __sk_buff *skb)
{
	struct traffic *counters = traffic_of(skb);
	if (counters)
		counters->transmitted += skb->len;
	return LET_PACKET_PASS;
}

char LICENSE[] SEC("license") = "GPL";
