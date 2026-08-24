#include "cloakframe/OrtAcceleration.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

#if defined(__APPLE__)
#include <QDateTime>
#include <QDir>
#include <QStandardPaths>
#endif

namespace cloakframe
{
    namespace
    {
#if defined(__APPLE__)
        // Compiled-model cache directories are content-addressed by model hash, so an entry
        // left behind by a removed or updated model is never read again; anything but the
        // active tag that has not been written to for this long is deleted.
        constexpr int kCoremlCachePruneDays = 60;

        QString coremlModelCacheDirectory(const QString &modelCacheTag)
        {
            const QString cacheBase =
                QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
            if (cacheBase.isEmpty() || modelCacheTag.isEmpty())
            {
                return {};
            }
            QDir root(cacheBase + QStringLiteral("/coreml-models"));
            if (!root.mkpath(modelCacheTag))
            {
                return {};
            }
            const QDateTime cutoff =
                QDateTime::currentDateTimeUtc().addDays(-kCoremlCachePruneDays);
            const auto siblings = root.entryInfoList(QDir::Dirs | QDir::NoDotAndDotDot);
            for (const auto &sibling : siblings)
            {
                if (sibling.fileName() != modelCacheTag && sibling.lastModified() < cutoff)
                {
                    QDir(sibling.absoluteFilePath()).removeRecursively();
                }
            }
            return root.filePath(modelCacheTag);
        }
#endif

        bool providerAvailable(const char *providerName)
        {
            const auto providers = Ort::GetAvailableProviders();
            return std::ranges::find(providers, std::string(providerName)) != providers.end();
        }

        OrtAccelerator applyOrtAcceleration(Ort::SessionOptions &options,
            const bool enabled,
            [[maybe_unused]] const QString &modelCacheTag)
        {
            if (!enabled)
            {
                return OrtAccelerator::None;
            }

#if defined(__APPLE__)
            if (providerAvailable("CoreMLExecutionProvider"))
            {
                try
                {
                    std::unordered_map<std::string, std::string> coremlOptions = {
                        {"ModelFormat", "MLProgram"},
                        {"MLComputeUnits", "ALL"},
                    };
                    const QString cacheDirectory = coremlModelCacheDirectory(modelCacheTag);
                    if (!cacheDirectory.isEmpty())
                    {
                        coremlOptions.emplace("ModelCacheDirectory", cacheDirectory.toStdString());
                        spdlog::info(
                            "CoreML compiled-model cache: {}", cacheDirectory.toStdString());
                    }
                    options.AppendExecutionProvider("CoreML", coremlOptions);
                    return OrtAccelerator::CoreML;
                }
                catch (const Ort::Exception &error)
                {
                    spdlog::warn(
                        "CoreML acceleration unavailable, falling back to CPU: {}", error.what());
                }
            }
            else
            {
                spdlog::info(
                    "CoreML execution provider not present in this ONNX Runtime build; using CPU.");
            }
#elif defined(_WIN32)
            if (providerAvailable("DmlExecutionProvider"))
            {
                try
                {
                    options.DisableMemPattern();
                    options.SetExecutionMode(ORT_SEQUENTIAL);
                    options.AppendExecutionProvider(
                        "DML", std::unordered_map<std::string, std::string>{});
                    return OrtAccelerator::DirectML;
                }
                catch (const Ort::Exception &error)
                {
                    options.EnableMemPattern();
                    spdlog::warn(
                        "DirectML acceleration unavailable, falling back to CPU: {}", error.what());
                }
            }
            else
            {
                spdlog::info("DirectML execution provider not present in this ONNX Runtime build; "
                             "using CPU.");
            }
#elif defined(__linux__)
            if (providerAvailable("CUDAExecutionProvider"))
            {
                try
                {
                    OrtCUDAProviderOptions cudaOptions;
                    options.AppendExecutionProvider_CUDA(cudaOptions);
                    return OrtAccelerator::CUDA;
                }
                catch (const Ort::Exception &error)
                {
                    spdlog::warn("CUDA acceleration unavailable: {}", error.what());
                }
            }

            if (providerAvailable("MIGraphXExecutionProvider"))
            {
                try
                {
                    OrtMIGraphXProviderOptions migraphxOptions{};
                    migraphxOptions.migraphx_mem_limit = SIZE_MAX;
                    options.AppendExecutionProvider_MIGraphX(migraphxOptions);
                    return OrtAccelerator::MIGraphX;
                }
                catch (const Ort::Exception &error)
                {
                    spdlog::warn("MIGraphX acceleration unavailable: {}", error.what());
                }
            }

            if (providerAvailable("ROCMExecutionProvider"))
            {
                try
                {
                    OrtROCMProviderOptions rocmOptions;
                    options.AppendExecutionProvider_ROCM(rocmOptions);
                    return OrtAccelerator::ROCm;
                }
                catch (const Ort::Exception &error)
                {
                    spdlog::warn("ROCm acceleration unavailable: {}", error.what());
                }
            }

            spdlog::info("No supported Linux GPU execution provider is available; using CPU.");
#endif
            return OrtAccelerator::None;
        }
    }

    const char *ortAcceleratorName(OrtAccelerator accelerator)
    {
        switch (accelerator)
        {
        case OrtAccelerator::CoreML:
            return "CoreML";
        case OrtAccelerator::DirectML:
            return "DirectML";
        case OrtAccelerator::CUDA:
            return "CUDA";
        case OrtAccelerator::MIGraphX:
            return "MIGraphX";
        case OrtAccelerator::ROCm:
            return "ROCm";
        case OrtAccelerator::None:
            break;
        }
        return "CPU";
    }

    QString ortModelCacheTag(
        const std::vector<std::uint8_t> &modelBytes, const QByteArray &expectedSha256)
    {
        if (!expectedSha256.isEmpty())
        {
            return QString::fromLatin1(expectedSha256.toHex());
        }
        QCryptographicHash hash(QCryptographicHash::Sha256);
        hash.addData(QByteArrayView(reinterpret_cast<const char *>(modelBytes.data()),
            static_cast<qsizetype>(modelBytes.size())));
        return QString::fromLatin1(hash.result().toHex());
    }

    OrtAccelerator configureOrtSessionOptions(
        Ort::SessionOptions &options, const bool enableAcceleration, const QString &modelCacheTag)
    {
        const OrtAccelerator accelerator =
            applyOrtAcceleration(options, enableAcceleration, modelCacheTag);
        // Layout optimizations (the step from EXTENDED to ALL) only help nodes the CPU
        // provider executes, and can rewrite nodes into forms an accelerator's provider no
        // longer claims, so ALL is reserved for CPU-only sessions.
        options.SetGraphOptimizationLevel(accelerator == OrtAccelerator::None
                                              ? GraphOptimizationLevel::ORT_ENABLE_ALL
                                              : GraphOptimizationLevel::ORT_ENABLE_EXTENDED);
        options.SetIntraOpNumThreads(
            accelerator == OrtAccelerator::None
                ? static_cast<int>(std::max(1U, std::thread::hardware_concurrency()))
                : 1);
        return accelerator;
    }
}
