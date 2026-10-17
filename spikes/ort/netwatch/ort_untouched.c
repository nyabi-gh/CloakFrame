// Creates an ONNX Runtime environment and session the way 1.12.1 does (Ort::Env, no telemetry
// call either way), runs YOLO5Face once and keeps the environment alive, so the library's own
// telemetry default can be observed with watch.sh.
//
//   clang -O2 -I <ort include dir> -o ort_untouched ort_untouched.c
//   ort_untouched <libonnxruntime.dylib> <yolov5n_face.onnx> <linger seconds> [disable]
//
// With "disable", DisableTelemetryEvents is called right after CreateEnv.

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "onnxruntime_c_api.h"

#define CHECK(expr)                                                         \
    do                                                                      \
    {                                                                       \
        OrtStatus *status = (expr);                                         \
        if (status)                                                         \
        {                                                                   \
            fprintf(stderr, "%s: %s\n", #expr, api->GetErrorMessage(status)); \
            return 1;                                                       \
        }                                                                   \
    } while (0)

int main(int argc, char **argv)
{
    if (argc != 4 && argc != 5)
    {
        fprintf(stderr, "usage: %s <libonnxruntime> <model> <linger seconds> [disable]\n", argv[0]);
        return 2;
    }
    void *lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!lib)
    {
        fprintf(stderr, "%s\n", dlerror());
        return 1;
    }
    const OrtApiBase *(*get_base)(void) = (const OrtApiBase *(*)(void))dlsym(lib, "OrtGetApiBase");
    const OrtApiBase *base = get_base();
    // 1.12.1 bundles ORT 1.29 and uses its C++ API; API 17 covers every call made here.
    const OrtApi *api = base->GetApi(17);
    printf("ORT %s\n", base->GetVersionString());

    OrtEnv *env = NULL;
    CHECK(api->CreateEnv(ORT_LOGGING_LEVEL_WARNING, "CloakFrame-YOLO5Face", &env));
    if (argc == 5)
    {
        CHECK(api->DisableTelemetryEvents(env));
    }
    OrtSessionOptions *options = NULL;
    CHECK(api->CreateSessionOptions(&options));
    OrtSession *session = NULL;
    CHECK(api->CreateSession(env, argv[2], options, &session));

    OrtMemoryInfo *memory = NULL;
    CHECK(api->CreateCpuMemoryInfo(OrtArenaAllocator, OrtMemTypeDefault, &memory));
    size_t count = 3 * 640 * 640;
    float *input = calloc(count, sizeof(float));
    int64_t shape[4] = {1, 3, 640, 640};
    OrtValue *tensor = NULL;
    CHECK(api->CreateTensorWithDataAsOrtValue(memory, input, count * sizeof(float), shape, 4,
        ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT, &tensor));
    OrtAllocator *allocator = NULL;
    CHECK(api->GetAllocatorWithDefaultOptions(&allocator));
    char *input_name = NULL;
    char *output_name = NULL;
    CHECK(api->SessionGetInputName(session, 0, allocator, &input_name));
    CHECK(api->SessionGetOutputName(session, 0, allocator, &output_name));
    for (int i = 0; i < 5; ++i)
    {
        OrtValue *output = NULL;
        const char *in[] = {input_name};
        const char *out[] = {output_name};
        CHECK(api->Run(session, NULL, in, (const OrtValue *const *)&tensor, 1, out, 1, &output));
        api->ReleaseValue(output);
    }
    printf("ran 5 inferences, lingering %s s\n", argv[3]);
    fflush(stdout);
    sleep((unsigned)atoi(argv[3]));
    api->ReleaseSession(session);
    api->ReleaseEnv(env);
    return 0;
}
