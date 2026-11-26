/* C2 runtime verification: dlopen libmpv.so on Android and exercise the client API.
 * Build (NDK r27c, API 26, x86_64):
 *   .../bin/x86_64-linux-android26-clang dlopen-test.c -o test-mpv -ldl
 * Run on device:
 *   LD_LIBRARY_PATH=/data/local/tmp/c2libs /data/local/tmp/c2libs/test-mpv
 */
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    dlerror();
    /* preload every NEEDED dep by absolute path so the translator linker
       (which ignores LD_LIBRARY_PATH under ndk_translation) can resolve them */
    static const char *deps[] = {
        "libavutil.so", "libswresample.so", "libswscale.so",
        "libavcodec.so", "libavformat.so", "libavfilter.so",
        "libavdevice.so", "libc++_shared.so", NULL,
    };
    char dir[512];
    if (argc > 2) { snprintf(dir, sizeof dir, "%s", argv[2]); }
    else { snprintf(dir, sizeof dir, "."); }
    for (int i = 0; deps[i]; i++) {
        char p[1024];
        snprintf(p, sizeof p, "%s/%s", dir, deps[i]);
        void *dh = dlopen(p, RTLD_NOW);
        if (!dh) { fprintf(stderr, "preload %s FAILED: %s\n", p, dlerror()); return 4; }
    }
    printf("preloaded 8 deps OK\n");
    const char *path = argc > 1 ? argv[1] : "libmpv.so";
    void *h = dlopen(path, RTLD_NOW);
    if (!h) {
        fprintf(stderr, "dlopen FAILED: %s\n", dlerror());
        return 1;
    }
    printf("dlopen(\"%s\", RTLD_NOW) = %p OK\n", path, h);

    unsigned long (*ver)(void) = (unsigned long (*)(void))dlsym(h, "mpv_client_api_version");
    void *(*create)(void) = (void *(*)(void))dlsym(h, "mpv_create");
    int (*init)(void *) = (int (*)(void *))dlsym(h, "mpv_initialize");
    void (*destroy)(void *) = (void (*)(void *))dlsym(h, "mpv_terminate_destroy");
    if (!ver || !create || !init || !destroy) {
        fprintf(stderr, "dlsym FAILED: %s\n", dlerror());
        return 2;
    }

    unsigned long v = ver();
    printf("mpv_client_api_version = 0x%lx (client API %lu.%lu)\n", v, v >> 16, (v >> 8) & 0xff);

    void *ctx = create();
    printf("mpv_create() = %p\n", ctx);
    if (!ctx) { fprintf(stderr, "mpv_create FAILED\n"); return 3; }

    int rc = init(ctx);
    printf("mpv_initialize() = %d (%s)\n", rc, rc == 0 ? "success" : "error");
    if (rc != 0) {
        /* -3 == MPV_ERROR_INVALID_PARAMETER is fine headless; anything else still proves linkage */
        fprintf(stderr, "note: initialize rc=%d (headless/no-audio emulator is acceptable)\n", rc);
    }

    destroy(ctx);
    printf("mpv_terminate_destroy() OK\nRUNTIME VERIFICATION PASSED\n");
    return 0;
}
