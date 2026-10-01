#pragma once

#include <opencv2/core.hpp>

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <string>
#include <vector>

class QImageReader;

namespace cloakframe
{
    enum class ImageFormat
    {
        Jpeg,
        Png,
        Bmp,
        Tiff,
        Webp,
    };

    // The supported format the file's leading bytes identify, whatever its name says, or nothing
    // for any other content. Qt's image reader and OpenCV both pick a decoder from the content
    // among every format they were built with, so this is what keeps an input away from the
    // decoders CloakFrame does not need.
    [[nodiscard]] std::optional<ImageFormat> sniffImageFormat(const std::filesystem::path &path);

    // Points `reader` at the decoder for `format` only, with content-based detection off.
    void restrictImageReader(QImageReader &reader, ImageFormat format);

    enum class ImageWriteResult
    {
        Failed,
        Saved,
        SavedWithoutMetadata,
    };

    enum class FileMoveResult
    {
        Moved,
        CrossDevice,
        Failed,
    };

    // On Windows the change time stays zero. Security software moves it when it stamps a file
    // it has scanned, and any writer can set it just like the write time, so comparing it only
    // adds failures.
    struct FileIdentity
    {
        std::uint64_t device = 0;
        std::uint64_t file = 0;
        std::uint64_t size = 0;
        std::int64_t modifiedSeconds = 0;
        std::int64_t modifiedNanoseconds = 0;
        std::int64_t changedSeconds = 0;
        std::int64_t changedNanoseconds = 0;

        bool operator==(const FileIdentity &) const = default;
    };

    [[nodiscard]] std::optional<FileIdentity> captureFileIdentity(
        const std::filesystem::path &path);

    // Test seam for the POSIX publication fallback used on filesystems that support neither an
    // exclusive rename nor hardlinks (FAT, exFAT, some network and FUSE mounts). No filesystem
    // reachable from the test suite lacks both, so this is the only way to exercise that path.
    // A no-op on Windows.
    void setAtomicPublicationDisabledForTesting(bool disabled);

    bool metadataSupportAvailable();

    int readExifOrientation(const std::filesystem::path &source);

    void applyOrientation(cv::Mat &image, int orientation);

    cv::Mat toDetectionBgr(const cv::Mat &image);

    cv::Mat imreadUnicode(const std::filesystem::path &source, int flags);

    [[nodiscard]] std::size_t imageFrameCount(const std::filesystem::path &source);

    bool imwriteUnicodeNoReplace(const std::filesystem::path &destination,
        const cv::Mat &image,
        const std::vector<int> &params = {});

    ImageWriteResult imwriteUnicodeNoReplaceAtRoot(const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const cv::Mat &image,
        const std::vector<int> &params = {},
        const std::filesystem::path &metadataSource = {},
        const std::function<bool()> &publishGuard = {});

    bool copyFileNoReplace(
        const std::filesystem::path &source, const std::filesystem::path &destination);

    bool copyFileNoReplaceAtRoot(const std::filesystem::path &source,
        const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const std::function<bool()> &publishGuard = {});

    FileMoveResult moveFileNoReplaceAtRoot(const std::filesystem::path &source,
        const std::filesystem::path &outputRoot,
        const std::filesystem::path &relativeDestination,
        const std::function<bool()> &publishGuard = {});

    std::vector<int> encodeParamsForExtension(const std::string &extLower);

    bool copyMetadata(const std::filesystem::path &source,
        const std::filesystem::path &destination,
        bool normalizeOrientation);
}
