// Interposed into a process with DYLD_INSERT_LIBRARIES to log every attempt to reach the
// network: BSD sockets, the resolver, DNS-SD and Network.framework endpoints. NSURLSession
// and the 1DS SDK in ONNX Runtime end up in one of these.
//
//   clang -dynamiclib -o libnetwatch.dylib netwatch.c -framework Network
//   NETWATCH_LOG=/path/log DYLD_INSERT_LIBRARIES=libnetwatch.dylib <program>

#include <dlfcn.h>
#include <dns_sd.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <network/network.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#define DYLD_INTERPOSE(replacement, replacee) \
    __attribute__((used)) static struct { const void *r; const void *e; } interpose_##replacee \
        __attribute__((section("__DATA,__interpose"))) = {(const void *)(unsigned long)&replacement, (const void *)(unsigned long)&replacee};

static void note(const char *fmt, ...)
{
    const char *path = getenv("NETWATCH_LOG");
    char line[512];
    va_list args;
    va_start(args, fmt);
    int n = vsnprintf(line, sizeof line - 2, fmt, args);
    va_end(args);
    if (n < 0)
    {
        return;
    }
    if (n > (int)sizeof line - 2)
    {
        n = sizeof line - 2;
    }
    line[n++] = '\n';
    int fd = path ? open(path, O_WRONLY | O_CREAT | O_APPEND, 0644) : 2;
    if (fd >= 0)
    {
        write(fd, line, (size_t)n);
        if (path)
        {
            close(fd);
        }
    }
}

static void describe(const struct sockaddr *addr, char *out, size_t size)
{
    if (!addr)
    {
        snprintf(out, size, "(null)");
    }
    else if (addr->sa_family == AF_INET)
    {
        char ip[INET_ADDRSTRLEN];
        const struct sockaddr_in *in = (const struct sockaddr_in *)addr;
        inet_ntop(AF_INET, &in->sin_addr, ip, sizeof ip);
        snprintf(out, size, "%s:%d", ip, ntohs(in->sin_port));
    }
    else if (addr->sa_family == AF_INET6)
    {
        char ip[INET6_ADDRSTRLEN];
        const struct sockaddr_in6 *in6 = (const struct sockaddr_in6 *)addr;
        inet_ntop(AF_INET6, &in6->sin6_addr, ip, sizeof ip);
        snprintf(out, size, "[%s]:%d", ip, ntohs(in6->sin6_port));
    }
    else if (addr->sa_family == AF_UNIX)
    {
        snprintf(out, size, "unix:%s", ((const struct sockaddr_un *)addr)->sun_path);
    }
    else
    {
        snprintf(out, size, "family %d", addr->sa_family);
    }
}

static int nw_socket(int domain, int type, int protocol)
{
    int fd = socket(domain, type, protocol);
    if (domain == AF_INET || domain == AF_INET6)
    {
        note("socket domain=%d type=%d -> %d", domain, type, fd);
    }
    return fd;
}
DYLD_INTERPOSE(nw_socket, socket)

static int nw_connect(int fd, const struct sockaddr *addr, socklen_t len)
{
    char where[128];
    describe(addr, where, sizeof where);
    if (!addr || addr->sa_family != AF_UNIX)
    {
        note("connect fd=%d %s", fd, where);
    }
    return connect(fd, addr, len);
}
DYLD_INTERPOSE(nw_connect, connect)

static ssize_t nw_sendto(int fd, const void *buf, size_t len, int flags, const struct sockaddr *addr, socklen_t alen)
{
    if (addr && addr->sa_family != AF_UNIX)
    {
        char where[128];
        describe(addr, where, sizeof where);
        note("sendto fd=%d %s bytes=%zu", fd, where, len);
    }
    return sendto(fd, buf, len, flags, addr, alen);
}
DYLD_INTERPOSE(nw_sendto, sendto)

static int nw_getaddrinfo(const char *host, const char *service, const struct addrinfo *hints, struct addrinfo **res)
{
    note("getaddrinfo host=%s service=%s", host ? host : "(null)", service ? service : "(null)");
    return getaddrinfo(host, service, hints, res);
}
DYLD_INTERPOSE(nw_getaddrinfo, getaddrinfo)

static DNSServiceErrorType nw_DNSServiceGetAddrInfo(DNSServiceRef *ref, DNSServiceFlags flags, uint32_t iface,
    DNSServiceProtocol proto, const char *host, DNSServiceGetAddrInfoReply cb, void *ctx)
{
    note("DNSServiceGetAddrInfo host=%s", host ? host : "(null)");
    return DNSServiceGetAddrInfo(ref, flags, iface, proto, host, cb, ctx);
}
DYLD_INTERPOSE(nw_DNSServiceGetAddrInfo, DNSServiceGetAddrInfo)

static DNSServiceErrorType nw_DNSServiceQueryRecord(DNSServiceRef *ref, DNSServiceFlags flags, uint32_t iface,
    const char *name, uint16_t rrtype, uint16_t rrclass, DNSServiceQueryRecordReply cb, void *ctx)
{
    note("DNSServiceQueryRecord name=%s type=%u", name ? name : "(null)", rrtype);
    return DNSServiceQueryRecord(ref, flags, iface, name, rrtype, rrclass, cb, ctx);
}
DYLD_INTERPOSE(nw_DNSServiceQueryRecord, DNSServiceQueryRecord)

static nw_endpoint_t nw_nw_endpoint_create_host(const char *host, const char *port)
{
    note("nw_endpoint_create_host host=%s port=%s", host ? host : "(null)", port ? port : "(null)");
    return nw_endpoint_create_host(host, port);
}
DYLD_INTERPOSE(nw_nw_endpoint_create_host, nw_endpoint_create_host)

static nw_endpoint_t nw_nw_endpoint_create_url(const char *url)
{
    note("nw_endpoint_create_url url=%s", url ? url : "(null)");
    return nw_endpoint_create_url(url);
}
DYLD_INTERPOSE(nw_nw_endpoint_create_url, nw_endpoint_create_url)

static nw_connection_t nw_nw_connection_create(nw_endpoint_t endpoint, nw_parameters_t parameters)
{
    note("nw_connection_create");
    return nw_connection_create(endpoint, parameters);
}
DYLD_INTERPOSE(nw_nw_connection_create, nw_connection_create)

__attribute__((constructor)) static void loaded(void)
{
    note("netwatch loaded pid=%d", getpid());
}
