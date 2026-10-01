#include "cloakframe/Mosaic.hpp"

#include <opencv2/core/version.hpp>
#if CV_VERSION_MAJOR >= 5
#include <opencv2/geometry/2d.hpp>
#endif
#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <exception>
#include <future>
#include <list>
#include <map>
#include <mutex>
#include <tuple>
#include <vector>

namespace cloakframe
{
    namespace
    {
        cv::Rect paddedRegion(const cv::Rect2f &box, int width, int height, float paddingRatio)
        {
            const double padding = std::isfinite(paddingRatio)
                                       ? std::max(0.0, static_cast<double>(paddingRatio))
                                       : 0.0;
            const double padX = static_cast<double>(box.width) * padding;
            const double padY = static_cast<double>(box.height) * padding;
            const double leftValue =
                std::clamp(static_cast<double>(box.x) - padX, 0.0, static_cast<double>(width));
            const double topValue =
                std::clamp(static_cast<double>(box.y) - padY, 0.0, static_cast<double>(height));
            const double rightValue = std::clamp(
                static_cast<double>(box.x) + box.width + padX, 0.0, static_cast<double>(width));
            const double bottomValue = std::clamp(
                static_cast<double>(box.y) + box.height + padY, 0.0, static_cast<double>(height));

            int x = static_cast<int>(std::floor(leftValue));
            int y = static_cast<int>(std::floor(topValue));
            int right = static_cast<int>(std::ceil(rightValue));
            int bottom = static_cast<int>(std::ceil(bottomValue));

            x = std::clamp(x, 0, width - 1);
            y = std::clamp(y, 0, height - 1);
            right = std::clamp(right, x + 1, width);
            bottom = std::clamp(bottom, y + 1, height);

            return cv::Rect(x, y, right - x, bottom - y);
        }

        constexpr int kMaxMosaicCells = 12;

        void mosaicRegion(cv::Mat roi, int blockSize)
        {
            const int widthCap = std::min(kMaxMosaicCells, std::max(1, roi.cols / 2));
            const int heightCap = std::min(kMaxMosaicCells, std::max(1, roi.rows / 2));
            const int smallWidth = std::clamp(roi.cols / blockSize, 1, widthCap);
            const int smallHeight = std::clamp(roi.rows / blockSize, 1, heightCap);

            cv::Mat small;
            cv::resize(roi, small, cv::Size(smallWidth, smallHeight), 0.0, 0.0, cv::INTER_LINEAR);
            cv::resize(small, roi, roi.size(), 0.0, 0.0, cv::INTER_NEAREST);
        }

        int oddKernelFor(double sigma)
        {
            int kernel = static_cast<int>(std::lround(sigma * 3.0)) | 1;
            return std::max(3, kernel);
        }

        void blurRegion(cv::Mat roi)
        {
            const int minEdge = std::min(roi.cols, roi.rows);
            if (minEdge < 2)
            {
                // Too thin to blur; a flat fill still hides it.
                roi.setTo(cv::mean(roi));
                return;
            }

            const double sigma = static_cast<double>(minEdge) / 6.0;
            int down = std::max(1, static_cast<int>(std::lround(sigma / 2.5)));
            down = std::min(down, minEdge / 2);
            if (down <= 1)
            {
                cv::GaussianBlur(
                    roi, roi, cv::Size(oddKernelFor(sigma), oddKernelFor(sigma)), sigma);
                return;
            }

            cv::Mat small;
            const cv::Size smallSize(std::max(1, roi.cols / down), std::max(1, roi.rows / down));
            cv::resize(roi, small, smallSize, 0.0, 0.0, cv::INTER_AREA);
            const double smallSigma = sigma / down;
            cv::GaussianBlur(small,
                small,
                cv::Size(oddKernelFor(smallSigma), oddKernelFor(smallSigma)),
                smallSigma);
            cv::resize(small, roi, roi.size(), 0.0, 0.0, cv::INTER_LINEAR);
        }

        double opaqueAlphaFor(const cv::Mat &image)
        {
            switch (image.depth())
            {
            case CV_16U:
                return 65535.0;
            case CV_32F:
            case CV_64F:
                return 1.0;
            default:
                return 255.0;
            }
        }

        void fillRegion(cv::Mat roi)
        {
            roi.setTo(cv::Scalar(0, 0, 0, opaqueAlphaFor(roi)));
        }

        int availableInsetFor(const cv::Rect &region, const cv::Rect &detected)
        {
            return std::max(0,
                std::min({detected.x - region.x,
                    detected.y - region.y,
                    region.x + region.width - detected.x - detected.width,
                    region.y + region.height - detected.y - detected.height}));
        }

        void blendWithMask(cv::Mat roi, const cv::Mat &anonymized, const cv::Mat &alpha);

        // An ellipse inscribed in the padded box misses the detected box's corners.
        void coverDetectedBox(cv::Mat &mask, const cv::Rect &detected)
        {
            const cv::Rect inside = detected & cv::Rect(0, 0, mask.cols, mask.rows);
            if (!inside.empty())
            {
                mask(inside).setTo(cv::Scalar(255));
            }
        }

        cv::Mat transformedCustomImage(const cv::Mat &customImage,
            const cv::Size &targetSize,
            const float rollRadians,
            const bool hasPose)
        {
            if (customImage.empty() || customImage.type() != CV_8UC4 || targetSize.width < 1
                || targetSize.height < 1)
            {
                return {};
            }

            cv::Mat premultiplied = customImage.clone();
            cv::parallel_for_(cv::Range(0, premultiplied.rows),
                [&](const cv::Range &range)
                {
                    for (int y = range.start; y < range.end; ++y)
                    {
                        auto *row = premultiplied.ptr<cv::Vec4b>(y);
                        for (int x = 0; x < premultiplied.cols; ++x)
                        {
                            const int alpha = row[x][3];
                            for (int channel = 0; channel < 3; ++channel)
                            {
                                row[x][channel] = static_cast<unsigned char>(
                                    (static_cast<int>(row[x][channel]) * alpha + 127) / 255);
                            }
                        }
                    }
                });

            const bool rotate =
                isValidFacePose(rollRadians, hasPose) && std::abs(rollRadians) > 0.001F;
            const double cosine = rotate ? std::abs(std::cos(rollRadians)) : 1.0;
            const double sine = rotate ? std::abs(std::sin(rollRadians)) : 0.0;
            const int overscan = rotate ? 2 : 0;
            const cv::Size canvasSize(std::max(targetSize.width,
                                          static_cast<int>(std::ceil(
                                              cosine * targetSize.width + sine * targetSize.height))
                                              + overscan),
                std::max(targetSize.height,
                    static_cast<int>(
                        std::ceil(sine * targetSize.width + cosine * targetSize.height))
                        + overscan));

            cv::Rect sourceRegion(0, 0, premultiplied.cols, premultiplied.rows);
            const double sourceAspect =
                static_cast<double>(premultiplied.cols) / premultiplied.rows;
            const double canvasAspect = static_cast<double>(canvasSize.width) / canvasSize.height;
            if (sourceAspect > canvasAspect)
            {
                sourceRegion.width =
                    std::clamp(static_cast<int>(std::lround(premultiplied.rows * canvasAspect)),
                        1,
                        premultiplied.cols);
                sourceRegion.x = (premultiplied.cols - sourceRegion.width) / 2;
            }
            else if (sourceAspect < canvasAspect)
            {
                sourceRegion.height =
                    std::clamp(static_cast<int>(std::lround(premultiplied.cols / canvasAspect)),
                        1,
                        premultiplied.rows);
                sourceRegion.y = (premultiplied.rows - sourceRegion.height) / 2;
            }

            cv::Mat canvas;
            const int interpolation =
                sourceRegion.width > canvasSize.width || sourceRegion.height > canvasSize.height
                    ? cv::INTER_AREA
                    : cv::INTER_LINEAR;
            cv::resize(premultiplied(sourceRegion), canvas, canvasSize, 0.0, 0.0, interpolation);

            cv::Mat rendered8;
            if (rotate)
            {
                constexpr double kRadiansToDegrees = 57.2957795131;
                const cv::Point2f sourceCenter(static_cast<float>(canvasSize.width - 1) * 0.5F,
                    static_cast<float>(canvasSize.height - 1) * 0.5F);
                const cv::Point2f targetCenter(static_cast<float>(targetSize.width - 1) * 0.5F,
                    static_cast<float>(targetSize.height - 1) * 0.5F);
                cv::Mat rotation = cv::getRotationMatrix2D(
                    sourceCenter, -static_cast<double>(rollRadians) * kRadiansToDegrees, 1.0);
                rotation.at<double>(0, 2) += targetCenter.x - sourceCenter.x;
                rotation.at<double>(1, 2) += targetCenter.y - sourceCenter.y;
                cv::warpAffine(canvas,
                    rendered8,
                    rotation,
                    targetSize,
                    cv::INTER_LINEAR,
                    cv::BORDER_CONSTANT,
                    cv::Scalar(0, 0, 0, 0));
            }
            else
            {
                rendered8 = std::move(canvas);
            }

            cv::parallel_for_(cv::Range(0, rendered8.rows),
                [&](const cv::Range &range)
                {
                    for (int y = range.start; y < range.end; ++y)
                    {
                        auto *row = rendered8.ptr<cv::Vec4b>(y);
                        for (int x = 0; x < rendered8.cols; ++x)
                        {
                            const int alpha = row[x][3];
                            if (alpha == 0)
                            {
                                row[x][0] = row[x][1] = row[x][2] = 0;
                                continue;
                            }
                            for (int channel = 0; channel < 3; ++channel)
                            {
                                row[x][channel] = cv::saturate_cast<unsigned char>(
                                    (static_cast<int>(row[x][channel]) * 255 + alpha / 2) / alpha);
                            }
                        }
                    }
                });
            return rendered8;
        }

        // cv::Mat is a refcounted header: taking it by value is how the pixels stay
        // shared and writable. A const reference here would only launder that away.
        void customImageRegion(
            // NOLINTNEXTLINE(performance-unnecessary-value-param)
            cv::Mat roi,
            const cv::Mat &customImage,
            const float rollRadians,
            const bool hasPose)
        {
            cv::Mat transformed =
                transformedCustomImage(customImage, roi.size(), rollRadians, hasPose);
            if (transformed.empty())
            {
                return;
            }

            cv::Mat alpha;
            cv::extractChannel(transformed, alpha, 3);
            cv::Mat rendered8;
            switch (roi.channels())
            {
            case 1:
                cv::cvtColor(transformed, rendered8, cv::COLOR_BGRA2GRAY);
                break;
            case 3:
                cv::cvtColor(transformed, rendered8, cv::COLOR_BGRA2BGR);
                break;
            case 4:
                rendered8 = transformed;
                break;
            default:
                return;
            }

            double scale = 1.0;
            if (roi.depth() == CV_16U)
            {
                scale = 257.0;
            }
            else if (roi.depth() == CV_32F || roi.depth() == CV_64F)
            {
                scale = 1.0 / 255.0;
            }
            cv::Mat rendered;
            rendered8.convertTo(rendered, roi.type(), scale);
            if (roi.channels() == 4)
            {
                cv::Mat destinationAlpha;
                cv::extractChannel(roi, destinationAlpha, 3);
                cv::insertChannel(destinationAlpha, rendered, 3);
            }
            blendWithMask(roi, rendered, alpha);
        }

        constexpr float kInnerTransitionRatio = 0.12F;
        constexpr float kOuterTransitionRatio = 0.10F;
        constexpr int kMinOuterTransition = 4;
        constexpr int kSizeQuantum = 4;

        int outerTransitionFor(const cv::Rect &region)
        {
            const int base = std::min(region.width, region.height);
            return std::max(kMinOuterTransition,
                static_cast<int>(std::lround(static_cast<float>(base) * kOuterTransitionRatio)));
        }

        int innerTransitionFor(const cv::Rect &region, const cv::Rect &detected)
        {
            const int available = availableInsetFor(region, detected);
            const int target = static_cast<int>(std::lround(
                static_cast<float>(std::min(region.width, region.height)) * kInnerTransitionRatio));
            return std::min(available, target);
        }

        cv::Rect quantizeRegionSize(const cv::Rect &region, int quantum)
        {
            if (quantum <= 1)
            {
                return region;
            }
            const int qw = ((region.width + quantum - 1) / quantum) * quantum;
            const int qh = ((region.height + quantum - 1) / quantum) * quantum;
            return {region.x, region.y, qw, qh};
        }

        cv::Mat softTransitionMask(const cv::Size &size,
            const cv::Rect &core,
            MaskShape shape,
            int innerTransition,
            int outerTransition)
        {
            constexpr std::size_t maxDistanceTransformPixels = std::size_t{2} * 1024 * 1024;
            const auto pixelCount =
                static_cast<std::size_t>(size.width) * static_cast<std::size_t>(size.height);
            if (pixelCount > maxDistanceTransformPixels)
            {
                cv::Mat mask(size, CV_8UC1);
                const float transition = static_cast<float>(innerTransition + outerTransition);
                const float centerX =
                    static_cast<float>(core.x) + static_cast<float>(core.width - 1) * 0.5F;
                const float centerY =
                    static_cast<float>(core.y) + static_cast<float>(core.height - 1) * 0.5F;
                const float radiusX = std::max(0.5F, static_cast<float>(core.width) * 0.5F);
                const float radiusY = std::max(0.5F, static_cast<float>(core.height) * 0.5F);
                const float radiusScale = std::min(radiusX, radiusY);

                cv::parallel_for_(cv::Range(0, size.height),
                    [&](const cv::Range &range)
                    {
                        for (int y = range.start; y < range.end; ++y)
                        {
                            auto *row = mask.ptr<unsigned char>(y);
                            for (int x = 0; x < size.width; ++x)
                            {
                                float signedDistance = 0.0F;
                                if (shape == MaskShape::Ellipse)
                                {
                                    const float dx = (static_cast<float>(x) - centerX) / radiusX;
                                    const float dy = (static_cast<float>(y) - centerY) / radiusY;
                                    signedDistance =
                                        (1.0F - std::sqrt(dx * dx + dy * dy)) * radiusScale;
                                }
                                else
                                {
                                    const float outsideX = std::max({static_cast<float>(core.x - x),
                                        0.0F,
                                        static_cast<float>(x - (core.x + core.width - 1))});
                                    const float outsideY = std::max({static_cast<float>(core.y - y),
                                        0.0F,
                                        static_cast<float>(y - (core.y + core.height - 1))});
                                    if (outsideX > 0.0F || outsideY > 0.0F)
                                    {
                                        signedDistance =
                                            -std::sqrt(outsideX * outsideX + outsideY * outsideY);
                                    }
                                    else
                                    {
                                        signedDistance =
                                            static_cast<float>(std::min({x - core.x + 1,
                                                y - core.y + 1,
                                                core.x + core.width - x,
                                                core.y + core.height - y}));
                                    }
                                }

                                float ramp = (signedDistance + static_cast<float>(outerTransition))
                                             / transition;
                                ramp = std::clamp(ramp, 0.0F, 1.0F);
                                ramp = ramp * ramp * (3.0F - 2.0F * ramp);
                                row[x] = cv::saturate_cast<unsigned char>(ramp * 255.0F);
                            }
                        }
                    });
                return mask;
            }

            cv::Mat inside(size, CV_8UC1, cv::Scalar(0));
            if (shape == MaskShape::Ellipse)
            {
                cv::ellipse(inside,
                    cv::Point(core.x + core.width / 2, core.y + core.height / 2),
                    cv::Size(core.width / 2, core.height / 2),
                    0.0,
                    0.0,
                    360.0,
                    cv::Scalar(255),
                    cv::FILLED,
                    cv::LINE_8);
            }
            else
            {
                cv::rectangle(inside, core, cv::Scalar(255), cv::FILLED);
            }

            cv::Mat outside;
            cv::bitwise_not(inside, outside);
            cv::Mat insideDistance;
            cv::Mat outsideDistance;
            cv::distanceTransform(inside, insideDistance, cv::DIST_L2, cv::DIST_MASK_PRECISE);
            cv::distanceTransform(outside, outsideDistance, cv::DIST_L2, cv::DIST_MASK_PRECISE);

            const float transition = static_cast<float>(innerTransition + outerTransition);
            cv::Mat ramp = (insideDistance - outsideDistance + static_cast<float>(outerTransition))
                           / transition;
            ramp = cv::min(ramp, 1.0F);
            ramp = cv::max(ramp, 0.0F);
            return ramp.mul(ramp).mul(3.0F - 2.0F * ramp);
        }

        using MaskKey = std::tuple<int, int, int, int, int, int, int, int, int>;

        struct MaskCacheEntry
        {
            cv::Mat mask;
            std::size_t bytes = 0;
            std::list<MaskKey>::iterator recency;
        };

        struct MaskCache
        {
            std::mutex mutex;
            std::map<MaskKey, MaskCacheEntry> entries;
            std::list<MaskKey> recency;
            std::size_t bytes = 0;
            // Masks being computed right now. A thread that needs one of them waits for it
            // instead of computing it again; masks with different keys are computed in parallel.
            std::map<MaskKey, std::shared_future<cv::Mat>> inFlight;
        };

        // Function-local so that a failure to construct it reaches the caller instead of
        // terminating the program during static initialization.
        MaskCache &maskCache()
        {
            static MaskCache cache;
            return cache;
        }
        constexpr std::size_t kMaskCacheEntryCap = 2048;
        constexpr std::size_t kMaskCacheByteCap = std::size_t{32} * 1024 * 1024;
        constexpr std::size_t kMaskCacheSingleEntryCap = std::size_t{8} * 1024 * 1024;

        // Called with the cache mutex held.
        void storeMask(MaskCache &cache, const MaskKey &key, const cv::Mat &mask)
        {
            const std::size_t maskBytes = mask.total() * mask.elemSize();
            if (maskBytes == 0 || maskBytes > kMaskCacheSingleEntryCap
                || cache.entries.contains(key))
            {
                return;
            }

            while (!cache.recency.empty()
                   && (cache.entries.size() >= kMaskCacheEntryCap
                       || cache.bytes > kMaskCacheByteCap - maskBytes))
            {
                const auto oldestKey = cache.recency.back();
                const auto oldest = cache.entries.find(oldestKey);
                if (oldest != cache.entries.end())
                {
                    cache.bytes -= oldest->second.bytes;
                    cache.entries.erase(oldest);
                }
                cache.recency.pop_back();
            }

            bool recencyAdded = false;
            try
            {
                cache.recency.push_front(key);
                recencyAdded = true;
                cache.entries.emplace(key, MaskCacheEntry{mask, maskBytes, cache.recency.begin()});
                cache.bytes += maskBytes;
            }
            catch (...)
            {
                if (recencyAdded)
                {
                    cache.recency.pop_front();
                }
            }
        }

        cv::Mat cachedSoftTransitionMask(const cv::Size &size,
            const cv::Rect &core,
            MaskShape shape,
            int innerTransition,
            int outerTransition)
        {
            const MaskKey key{size.width,
                size.height,
                core.x,
                core.y,
                core.width,
                core.height,
                static_cast<int>(shape),
                innerTransition,
                outerTransition};

            MaskCache &cache = maskCache();
            std::promise<cv::Mat> computation;
            {
                std::unique_lock<std::mutex> lock(cache.mutex);
                const auto cached = cache.entries.find(key);
                if (cached != cache.entries.end())
                {
                    cache.recency.splice(
                        cache.recency.begin(), cache.recency, cached->second.recency);
                    return cached->second.mask;
                }
                const auto running = cache.inFlight.find(key);
                if (running != cache.inFlight.end())
                {
                    const auto result = running->second;
                    lock.unlock();
                    return result.get();
                }
                cache.inFlight.emplace(key, computation.get_future().share());
            }

            cv::Mat mask;
            try
            {
                mask = softTransitionMask(size, core, shape, innerTransition, outerTransition);
            }
            catch (...)
            {
                {
                    const std::lock_guard<std::mutex> lock(cache.mutex);
                    cache.inFlight.erase(key);
                }
                computation.set_exception(std::current_exception());
                throw;
            }

            {
                const std::lock_guard<std::mutex> lock(cache.mutex);
                storeMask(cache, key, mask);
                cache.inFlight.erase(key);
            }
            computation.set_value(mask);
            return mask;
        }

        template <typename Pixel, typename Alpha>
        void blendWithMaskTyped(
            cv::Mat roi, const cv::Mat &anonymized, const cv::Mat &alpha, const double alphaScale)
        {
            const int channels = roi.channels();
            cv::parallel_for_(cv::Range(0, roi.rows),
                [&](const cv::Range &range)
                {
                    for (int y = range.start; y < range.end; ++y)
                    {
                        auto *destination = roi.ptr<Pixel>(y);
                        const auto *source = anonymized.ptr<Pixel>(y);
                        const auto *weights = alpha.ptr<Alpha>(y);
                        for (int x = 0; x < roi.cols; ++x)
                        {
                            const double weight = static_cast<double>(weights[x]) * alphaScale;
                            const int offset = x * channels;
                            for (int channel = 0; channel < channels; ++channel)
                            {
                                const double original =
                                    static_cast<double>(destination[offset + channel]);
                                const double changed =
                                    static_cast<double>(source[offset + channel]);
                                destination[offset + channel] = cv::saturate_cast<Pixel>(
                                    original + (changed - original) * weight);
                            }
                        }
                    }
                });
        }

        template <typename Pixel>
        // cv::Mat is a refcounted header: taking it by value is how the pixels stay
        // shared and writable. A const reference here would only launder that away.
        // NOLINTNEXTLINE(performance-unnecessary-value-param)
        void blendWithMaskDepth(cv::Mat roi, const cv::Mat &anonymized, const cv::Mat &alpha)
        {
            if (alpha.depth() == CV_8U)
            {
                blendWithMaskTyped<Pixel, unsigned char>(roi, anonymized, alpha, 1.0 / 255.0);
            }
            else
            {
                blendWithMaskTyped<Pixel, float>(roi, anonymized, alpha, 1.0);
            }
        }

        // cv::Mat is a refcounted header: taking it by value is how the pixels stay
        // shared and writable. A const reference here would only launder that away.
        // NOLINTNEXTLINE(performance-unnecessary-value-param)
        void blendWithMask(cv::Mat roi, const cv::Mat &anonymized, const cv::Mat &alpha)
        {
            switch (roi.depth())
            {
            case CV_8U:
                blendWithMaskDepth<unsigned char>(roi, anonymized, alpha);
                break;
            case CV_8S:
                blendWithMaskDepth<signed char>(roi, anonymized, alpha);
                break;
            case CV_16U:
                blendWithMaskDepth<unsigned short>(roi, anonymized, alpha);
                break;
            case CV_16S:
                blendWithMaskDepth<short>(roi, anonymized, alpha);
                break;
            case CV_32S:
                blendWithMaskDepth<int>(roi, anonymized, alpha);
                break;
            case CV_32F:
                blendWithMaskDepth<float>(roi, anonymized, alpha);
                break;
            case CV_64F:
                blendWithMaskDepth<double>(roi, anonymized, alpha);
                break;
            default:
                CV_Error(cv::Error::StsUnsupportedFormat, "Unsupported image depth");
            }
        }
    }

    // cv::Mat is a refcounted header: taking it by value is how the pixels stay shared
    // and writable. A const reference here would only launder that away.
    // NOLINTNEXTLINE(performance-unnecessary-value-param)
    void applyEffect(cv::Mat roi, AnonymizationMethod method, int blockSize)
    {
        switch (method)
        {
        case AnonymizationMethod::Blur:
            blurRegion(roi);
            break;
        case AnonymizationMethod::Fill:
            fillRegion(roi);
            break;
        case AnonymizationMethod::CustomImage:
            break;
        case AnonymizationMethod::Mosaic:
        default:
            mosaicRegion(roi, blockSize);
            break;
        }
    }

    void applyAnonymization(cv::Mat &image,
        const FaceDetections &detections,
        AnonymizationMethod method,
        int blockSize,
        float paddingRatio,
        MaskShape shape,
        bool softEdges,
        const cv::Mat &customImage)
    {
        if (image.empty())
        {
            return;
        }

        const int width = image.cols;
        const int height = image.rows;

        blockSize = std::max(blockSize, 2);

        for (const auto &detection : detections)
        {
            if (!isValidFaceDetection(detection))
            {
                continue;
            }
            const cv::Rect roiRect = paddedRegion(detection.box, width, height, paddingRatio);

            if (method == AnonymizationMethod::CustomImage)
            {
                cv::Mat roi = image(roiRect);
                mosaicRegion(roi, blockSize);
                customImageRegion(roi, customImage, detection.rollRadians, detection.hasPose);
                continue;
            }

            if (softEdges)
            {
                const cv::Rect qRect = quantizeRegionSize(roiRect, kSizeQuantum);
                const cv::Rect detectedRect = paddedRegion(detection.box, width, height, 0.0F);
                const int innerTransition = innerTransitionFor(qRect, detectedRect);
                const int outerTransition = outerTransitionFor(qRect);
                const int canonX = qRect.x - outerTransition;
                const int canonY = qRect.y - outerTransition;

                const int outerX = std::max(0, canonX);
                const int outerY = std::max(0, canonY);
                const int outerRight = std::min(width, qRect.x + qRect.width + outerTransition);
                const int outerBottom = std::min(height, qRect.y + qRect.height + outerTransition);
                if (outerRight <= outerX || outerBottom <= outerY)
                {
                    continue;
                }
                const cv::Rect outer(outerX, outerY, outerRight - outerX, outerBottom - outerY);
                cv::Mat roi = image(outer);

                cv::Mat anonymized = roi.clone();
                applyEffect(anonymized, method, blockSize);

                const cv::Size canonicalSize(
                    qRect.width + 2 * outerTransition, qRect.height + 2 * outerTransition);
                const cv::Rect core(outerTransition, outerTransition, qRect.width, qRect.height);
                const cv::Mat canonical = cachedSoftTransitionMask(
                    canonicalSize, core, shape, innerTransition, outerTransition);
                const cv::Rect slice(outerX - canonX, outerY - canonY, outer.width, outer.height);
                if (shape == MaskShape::Ellipse)
                {
                    cv::Mat alpha = canonical(slice).clone();
                    coverDetectedBox(alpha, detectedRect - outer.tl());
                    blendWithMask(roi, anonymized, alpha);
                }
                else
                {
                    blendWithMask(roi, anonymized, canonical(slice));
                }
                continue;
            }

            cv::Mat roi = image(roiRect);

            if (shape == MaskShape::Ellipse)
            {
                cv::Mat masked = roi.clone();
                applyEffect(masked, method, blockSize);

                cv::Mat mask(roi.size(), CV_8UC1, cv::Scalar(0));
                cv::ellipse(mask,
                    cv::Point(roi.cols / 2, roi.rows / 2),
                    cv::Size(roi.cols / 2, roi.rows / 2),
                    0.0,
                    0.0,
                    360.0,
                    cv::Scalar(255),
                    cv::FILLED,
                    cv::LINE_AA);
                coverDetectedBox(
                    mask, paddedRegion(detection.box, width, height, 0.0F) - roiRect.tl());
                masked.copyTo(roi, mask);
            }
            else
            {
                applyEffect(roi, method, blockSize);
            }
        }
    }

    void applyMosaic(
        cv::Mat &image, const FaceDetections &detections, int blockSize, float paddingRatio)
    {
        applyAnonymization(image, detections, AnonymizationMethod::Mosaic, blockSize, paddingRatio);
    }
}
