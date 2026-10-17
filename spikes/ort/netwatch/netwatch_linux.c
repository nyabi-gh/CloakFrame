// Linux counterpart of netwatch.c, loaded with LD_PRELOAD: logs socket, connect, sendto, sendmsg
// and getaddrinfo calls. The 1DS SDK inside ONNX Runtime uses a static libcurl that still calls
// these libc functions, so they are seen even where strace is unavailable or unreliable.
//
//   gcc -O2 -shared -fPIC -o libnetwatch.so netwatch_linux.c -ldl
//   NETWATCH_LOG=/path/log LD_PRELOAD=./libnetwatch.so <program>

#define _GNU_SOURCE
#include <arpa/inet.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>

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
        ssize_t ignored = write(fd, line, (size_t)n);
        (void)ignored;
        if (path)
        {
            close(fd);
        }
    }
}

static int describe(const struct sockaddr *addr, char *out, size_t size)
{
    if (!addr)
    {
        return 0;
    }
    if (addr->sa_family == AF_INET)
    {
        const struct sockaddr_in *in = (const struct sockaddr_in *)addr;
        char ip[INET_ADDRSTRLEN];
        inet_ntop(AF_INET, &in->sin_addr, ip, sizeof ip);
        snprintf(out, size, "%s:%d", ip, ntohs(in->sin_port));
        return 1;
    }
    if (addr->sa_family == AF_INET6)
    {
        const struct sockaddr_in6 *in6 = (const struct sockaddr_in6 *)addr;
        char ip[INET6_ADDRSTRLEN];
        inet_ntop(AF_INET6, &in6->sin6_addr, ip, sizeof ip);
        snprintf(out, size, "[%s]:%d", ip, ntohs(in6->sin6_port));
        return 1;
    }
    return 0;
}

int socket(int domain, int type, int protocol)
{
    static int (*real)(int, int, int);
    if (!real)
    {
        real = dlsym(RTLD_NEXT, "socket");
    }
    int fd = real(domain, type, protocol);
    if (domain == AF_INET || domain == AF_INET6)
    {
        note("socket domain=%d type=%d -> %d", domain, type, fd);
    }
    return fd;
}

int connect(int fd, const struct sockaddr *addr, socklen_t len)
{
    static int (*real)(int, const struct sockaddr *, socklen_t);
    if (!real)
    {
        real = dlsym(RTLD_NEXT, "connect");
    }
    char where[128];
    if (describe(addr, where, sizeof where))
    {
        note("connect fd=%d %s", fd, where);
    }
    return real(fd, addr, len);
}

ssize_t sendto(int fd, const void *buf, size_t n, int flags, const struct sockaddr *addr, socklen_t len)
{
    static ssize_t (*real)(int, const void *, size_t, int, const struct sockaddr *, socklen_t);
    if (!real)
    {
        real = dlsym(RTLD_NEXT, "sendto");
    }
    char where[128];
    if (describe(addr, where, sizeof where))
    {
        note("sendto fd=%d %s bytes=%zu", fd, where, n);
    }
    return real(fd, buf, n, flags, addr, len);
}

ssize_t sendmsg(int fd, const struct msghdr *msg, int flags)
{
    static ssize_t (*real)(int, const struct msghdr *, int);
    if (!real)
    {
        real = dlsym(RTLD_NEXT, "sendmsg");
    }
    char where[128];
    if (msg && describe((const struct sockaddr *)msg->msg_name, where, sizeof where))
    {
        note("sendmsg fd=%d %s", fd, where);
    }
    return real(fd, msg, flags);
}

int getaddrinfo(const char *host, const char *service, const struct addrinfo *hints, struct addrinfo **res)
{
    static int (*real)(const char *, const char *, const struct addrinfo *, struct addrinfo **);
    if (!real)
    {
        real = dlsym(RTLD_NEXT, "getaddrinfo");
    }
    note("getaddrinfo host=%s service=%s", host ? host : "(null)", service ? service : "(null)");
    return real(host, service, hints, res);
}

__attribute__((constructor)) static void loaded(void)
{
    note("netwatch loaded pid=%d", getpid());
}
