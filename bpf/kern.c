#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>

struct {
	__uint(type, BPF_MAP_TYPE_XSKMAP);
	__type(key, __u32);
	__type(value, __u32);
	__uint(max_entries, 64);
} xsks_map SEC(".maps");

SEC("xdp")
int xdp_sock_prog(struct xdp_md *ctx)
{
    int index = ctx->rx_queue_index;

    const char log[] = "queue %d\n";
    bpf_trace_printk(log, sizeof(log), index);

    /* A set entry here means that the correspnding queue_id
     * has an active AF_XDP socket bound to it. */
    if (bpf_map_lookup_elem(&xsks_map, &index)) {
        long ret = bpf_redirect_map(&xsks_map, index, 0);

        const char s[] = "s %d\n";
        bpf_trace_printk(s, sizeof(s), ret);

        return ret;
    }


    const char log2[] = "after\n";
    bpf_trace_printk(log2, sizeof(log2));

    return XDP_PASS;
}

char _license[] SEC("license") = "GPL";