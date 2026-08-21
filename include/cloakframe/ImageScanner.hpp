#pragma once

#include <QString>
#include <QStringList>

#include <filesystem>
#include <system_error>
#include <vector>

namespace cloakframe
{
    struct ScanResult
    {
        std::filesystem::path sourcePath;
        std::filesystem::path relativePath;
    };

    // An input the scan could not read. Whatever is listed here is missing from the results,
    // so a caller that drops it reports a finished run over fewer files than the user chose.
    struct ScanIssue
    {
        std::filesystem::path path;
        std::error_code error;
    };

    std::vector<ScanResult> scanImages(
        const QStringList &inputs, bool recursive, std::vector<ScanIssue> *issues = nullptr);

    std::vector<ScanResult> scanMedia(const QStringList &inputs,
        bool recursive,
        bool includeVideos,
        std::vector<ScanIssue> *issues = nullptr);

    bool isSupportedImage(const std::filesystem::path &path);
}
