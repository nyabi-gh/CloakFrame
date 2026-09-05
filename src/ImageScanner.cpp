#include "cloakframe/ImageScanner.hpp"

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QFileInfo>

#include <algorithm>
#include <system_error>
#include <unordered_set>

namespace cloakframe
{
    namespace
    {
        std::string lowercaseExtension(const std::filesystem::path &path)
        {
            auto extension = pathToUtf8(path.extension());
            for (auto &ch : extension)
            {
                if (ch >= 'A' && ch <= 'Z')
                {
                    ch = static_cast<char>(ch - 'A' + 'a');
                }
            }
            return extension;
        }

        bool escapesBase(const std::filesystem::path &relative)
        {
            for (const auto &part : relative)
            {
                if (part == "..")
                {
                    return true;
                }
            }
            return false;
        }

        void appendFile(std::vector<ScanResult> &results,
            const std::filesystem::path &file,
            const std::filesystem::path &base,
            const bool includeVideos)
        {
            if (!isSupportedImage(file) && !(includeVideos && isSupportedVideo(file)))
            {
                return;
            }

            std::error_code error;
            auto relative = std::filesystem::relative(file, base, error);
            if (error || relative.empty() || relative.is_absolute() || escapesBase(relative))
            {
                relative = file.filename();
            }
            results.push_back({file, relative});
        }
    }

    bool isSupportedImage(const std::filesystem::path &path)
    {
        const auto extension = lowercaseExtension(path);
        return extension == ".jpg" || extension == ".jpeg" || extension == ".png"
               || extension == ".bmp" || extension == ".tif" || extension == ".tiff"
               || extension == ".webp";
    }

    std::vector<ScanResult> scanImages(
        const QStringList &inputs, bool recursive, std::vector<ScanIssue> *issues)
    {
        return scanMedia(inputs, recursive, false, issues);
    }

    std::vector<ScanResult> scanMedia(const QStringList &inputs,
        bool recursive,
        const bool includeVideos,
        std::vector<ScanIssue> *issues)
    {
        std::vector<ScanResult> results;

        const auto recordIssue =
            [issues](const std::filesystem::path &path, const std::error_code &error)
        {
            if (issues)
            {
                issues->push_back({path, error});
            }
        };

        std::unordered_set<std::string> visitedCanonical;
        const auto markVisited = [&visitedCanonical](const std::filesystem::path &file) -> bool
        {
            std::error_code ec;
            auto canonical = std::filesystem::canonical(file, ec);
            const auto key = ec ? pathToUtf8(file.lexically_normal()) : pathToUtf8(canonical);
            return visitedCanonical.insert(key).second;
        };

        for (const auto &input : inputs)
        {
            const QFileInfo info(input);
            const auto path = pathFromQString(input);

            if (info.isFile())
            {
                if (markVisited(path))
                {
                    appendFile(results, path, path.parent_path(), includeVideos);
                }
                continue;
            }

            if (!info.isDir())
            {
                std::error_code existsError;
                if (!std::filesystem::exists(path, existsError) || existsError)
                {
                    recordIssue(path,
                        existsError ? existsError
                                    : std::make_error_code(std::errc::no_such_file_or_directory));
                }
                continue;
            }

            std::vector<std::filesystem::path> pending{path};
            while (!pending.empty())
            {
                const auto directory = std::move(pending.back());
                pending.pop_back();
                std::error_code openError;
                std::filesystem::directory_iterator it(directory, openError);
                if (openError)
                {
                    recordIssue(directory, openError);
                    continue;
                }
                const std::filesystem::directory_iterator end;
                while (it != end)
                {
                    std::error_code entryError;
                    if (it->is_regular_file(entryError) && markVisited(it->path()))
                    {
                        appendFile(results, it->path(), path, includeVideos);
                    }
                    if (!entryError && recursive && it->is_directory(entryError)
                        && !it->is_symlink(entryError))
                    {
                        pending.push_back(it->path());
                    }
                    if (entryError)
                    {
                        recordIssue(it->path(), entryError);
                    }
                    std::error_code advanceError;
                    it.increment(advanceError);
                    if (advanceError)
                    {
                        recordIssue(directory, advanceError);
                        break;
                    }
                }
            }
        }

        std::ranges::sort(results,
            [](const ScanResult &a, const ScanResult &b)
            {
                return pathToUtf8(a.sourcePath) < pathToUtf8(b.sourcePath);
            });

        return results;
    }
}
