// LD_PRELOAD shim: make the listed DRM nodes unopenable so GPU drivers never touch them.
// Vulkan drivers (RADV) open and query every AMD node while enumerating devices, so selecting
// a device by ID is too late; refusing the open() keeps the hidden GPU completely untouched.
//
// G2_HIDE_DRI="renderD129:card1"   colon-separated basenames under /dev/dri to refuse (ENOENT)

#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>

static int hidden(const char *path) {
    const char *list = getenv("G2_HIDE_DRI");
    if (!path || !list || strncmp(path, "/dev/dri/", 9) != 0) {
        return 0;
    }
    const char *name = path + 9;
    size_t n = strlen(name);
    for (const char *p = list; *p;) {
        const char *end = strchr(p, ':');
        size_t len = end ? (size_t)(end - p) : strlen(p);
        if (len == n && strncmp(p, name, n) == 0) {
            return 1;
        }
        if (!end) {
            break;
        }
        p = end + 1;
    }
    return 0;
}

#define MODE_ARG                                                                                   \
    mode_t mode = 0;                                                                               \
    if (flags & (O_CREAT | O_TMPFILE)) {                                                           \
        va_list ap;                                                                                \
        va_start(ap, flags);                                                                       \
        mode = va_arg(ap, mode_t);                                                                 \
        va_end(ap);                                                                                \
    }

#define WRAP_OPEN(name)                                                                            \
    int name(const char *path, int flags, ...) {                                                   \
        MODE_ARG                                                                                   \
        if (hidden(path)) {                                                                        \
            errno = ENOENT;                                                                        \
            return -1;                                                                             \
        }                                                                                          \
        static int (*real)(const char *, int, ...);                                                \
        if (!real) real = dlsym(RTLD_NEXT, #name);                                                 \
        return real(path, flags, mode);                                                            \
    }

#define WRAP_OPENAT(name)                                                                          \
    int name(int dirfd, const char *path, int flags, ...) {                                        \
        MODE_ARG                                                                                   \
        if (hidden(path)) {                                                                        \
            errno = ENOENT;                                                                        \
            return -1;                                                                             \
        }                                                                                          \
        static int (*real)(int, const char *, int, ...);                                           \
        if (!real) real = dlsym(RTLD_NEXT, #name);                                                 \
        return real(dirfd, path, flags, mode);                                                     \
    }

WRAP_OPEN(open)
WRAP_OPEN(open64)
WRAP_OPENAT(openat)
WRAP_OPENAT(openat64)

// glibc's fortified entry points (used with _FORTIFY_SOURCE).
int __open_2(const char *path, int flags) { return open(path, flags); }
int __open64_2(const char *path, int flags) { return open64(path, flags); }
int __openat_2(int dirfd, const char *path, int flags) { return openat(dirfd, path, flags); }
int __openat64_2(int dirfd, const char *path, int flags) { return openat64(dirfd, path, flags); }
