#pragma once

#include <onnxruntime_cxx_api.h>

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

    // Applies the platform accelerator when enabled, then sets the graph-optimization and
    // threading policy that matches the execution provider actually in use.
    OrtAccelerator configureOrtSessionOptions(
        Ort::SessionOptions &options, bool enableAcceleration);
}
