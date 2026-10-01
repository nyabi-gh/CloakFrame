#pragma once

#include <QString>
#include <QStringList>

#include <filesystem>
#include <string>
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

    // Whether names that differ only in letter case reach the same file inside `directory`.
    // Read from the filesystem, not assumed from the operating system: macOS and Linux volumes
    // can be either, and Windows directories can be made case-sensitive. Checked read-only on
    // the nearest existing directory by looking a name up in the other case; when no name on
    // the way has a letter, the platform's usual default is returned.
    [[nodiscard]] bool namesFoldCase(const std::filesystem::path &directory);

    // A comparison key for `path`: its text, case-folded when `foldCase` is true.
    [[nodiscard]] std::string pathKey(const std::filesystem::path &path, bool foldCase);

    std::vector<ScanResult> scanImages(
        const QStringList &inputs, bool recursive, std::vector<ScanIssue> *issues = nullptr);

    std::vector<ScanResult> scanMedia(const QStringList &inputs,
        bool recursive,
        bool includeVideos,
        std::vector<ScanIssue> *issues = nullptr);

    bool isSupportedImage(const std::filesystem::path &path);
}
