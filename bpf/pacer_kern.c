#include <stddef.h>
#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/ipv6.h>

void* ptr_offset(struct xdp_md*, size_t, size_t);
long ipv4(struct xdp_md*);
long ipv6(struct xdp_md*);

struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 1UL << 20); // 1MiB of data stored
} packets SEC(".maps");

typedef enum addr_type {
    IPV4 = 0,
    IPV6
} t_addr_type;

typedef struct addr {
    __u32 ifindex;
    t_addr_type type;
    union {
        __u8 v4[4];
        __u8 v6[16];
    };
} t_addr;

SEC("xdp")
int xdp_pacer(struct xdp_md* ctx) {
    struct ethhdr* ethhdr;

    // Get ethernet header
    if ((ethhdr = ptr_offset(ctx, 0, sizeof(struct ethhdr))) == NULL) {
        return XDP_PASS;
    }

    // We only process IPv4 & IPv6 packets
    __u16 proto = bpf_ntohs(ethhdr->h_proto);
    if (!(proto == ETH_P_IP || proto == ETH_P_IPV6)) {
        return XDP_PASS;
    }

    switch (proto) {
        case ETH_P_IP:
            return ipv4(ctx);
        case ETH_P_IPV6:
            return ipv6(ctx);
        default:
            return XDP_PASS;
    }
}

inline long ipv4(struct xdp_md* ctx) {
    struct iphdr* iphdr;
    struct addr* ring_addr;
    t_addr_type type = 0;

    if ((iphdr = ptr_offset(ctx, sizeof(struct ethhdr), sizeof(struct iphdr))) == NULL) {
        return XDP_PASS;
    }

    if ((ring_addr = bpf_ringbuf_reserve(&packets, sizeof(struct addr), 0)) == NULL) {
        return XDP_PASS;
    }

    __builtin_memcpy(&ring_addr->ifindex, &ctx->ingress_ifindex, 4);
    __builtin_memcpy(&ring_addr->type, &type, sizeof(t_addr_type));
    __builtin_memcpy(&ring_addr->v4, &iphdr->saddr, 4);

    bpf_ringbuf_submit(ring_addr, 0);

    return XDP_PASS;
}

inline long ipv6(struct xdp_md* ctx) {
    struct ipv6hdr* ip6hdr;
    struct addr* ring_addr;
    t_addr_type type = 1;

    if ((ip6hdr = ptr_offset(ctx, sizeof(struct ethhdr), sizeof(struct ipv6hdr))) == NULL) {
        return XDP_PASS;
    }

    if ((ring_addr = bpf_ringbuf_reserve(&packets, sizeof(struct addr), 0)) == NULL) {
        return XDP_PASS;
    }

    __builtin_memcpy(&ring_addr->ifindex, &ctx->ingress_ifindex, 4);
    __builtin_memcpy(&ring_addr->type, &type, sizeof(t_addr_type));
    __builtin_memcpy(&ring_addr->v4, &ip6hdr->saddr, 16);

    bpf_ringbuf_submit(ring_addr, 0);

    return XDP_PASS;
}

/* Validate and return pointer from offset */
inline void* ptr_offset(struct xdp_md* ctx, size_t offset, size_t len)
{
    void* ptr = (void*)(long)ctx->data + offset;

    if (ptr + len > (void*)(long)ctx->data_end) {
        return NULL;
    }

    return ptr;
}

char _license[] SEC("license") = "GPL";
