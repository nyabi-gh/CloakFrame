#pragma once

#include <QByteArray>
#include <QString>

#include <onnxruntime_cxx_api.h>

#include <cstdint>
#include <vector>

namespace cloakframe
{
    enum class OrtAccelerator
    {
        None,
        CoreML,
        DirectML,
        CUDA,
        MIGraphX,
        ROCm,
    };

    [[nodiscard]] const char *ortAcceleratorName(OrtAccelerator accelerator);

    // Content-derived identity for a model's compiled-model cache: the SHA-256 of its bytes.
    // ONNX Runtime never invalidates the CoreML cache itself, and a session created from
    // memory keys it only on the model's input/output names, so distinct models must be
    // separated by directory or a stale compiled model would be reused silently.
    [[nodiscard]] QString ortModelCacheTag(
        const std::vector<std::uint8_t> &modelBytes, const QByteArray &expectedSha256);

    // Applies the platform accelerator when enabled, then sets the graph-optimization and
    // threading policy that matches the execution provider actually in use. A non-empty
    // modelCacheTag lets CoreML persist the compiled model across sessions; leave it empty
    // when several graph variants of one model share the options (their compiled models
    // would collide).
    OrtAccelerator configureOrtSessionOptions(
        Ort::SessionOptions &options, bool enableAcceleration, const QString &modelCacheTag = {});
}
