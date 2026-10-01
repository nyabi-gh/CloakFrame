#include "cloakframe/ImageScanner.hpp"

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QFileInfo>

#include <algorithm>
#include <system_error>
#include <unordered_map>
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

        bool isSystemFile(const std::filesystem::path &file)
        {
            const QString name = pathToQString(file.filename());
            return name.startsWith(QLatin1Char('.'))
                   || name.compare(QStringLiteral("Thumbs.db"), Qt::CaseInsensitive) == 0
                   || name.compare(QStringLiteral("desktop.ini"), Qt::CaseInsensitive) == 0;
        }

        void appendFile(std::vector<ScanResult> &results,
            const std::filesystem::path &file,
            const std::filesystem::path &base,
            const bool includeVideos,
            SkippedTypes *skipped)
        {
            if (!isSupportedImage(file) && !(includeVideos && isSupportedVideo(file)))
            {
                if (skipped != nullptr && !isSystemFile(file))
                {
                    ++(*skipped)[lowercaseExtension(file)];
                }
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

    bool namesFoldCase(const std::filesystem::path &directory)
    {
        std::error_code ec;
        auto probe = std::filesystem::absolute(directory, ec).lexically_normal();
        if (!ec && !probe.has_filename())
        {
            probe = probe.parent_path();
        }
        while (!ec && probe.has_filename())
        {
            std::error_code existsError;
            if (std::filesystem::exists(probe, existsError))
            {
                QString variant = pathToQString(probe.filename());
                const auto letter = std::ranges::find_if(variant,
                    [](const QChar ch)
                    {
                        return ch.isLower() || ch.isUpper();
                    });
                if (letter != variant.end())
                {
                    *letter = letter->isLower() ? letter->toUpper() : letter->toLower();
                    std::error_code equivalentError;
                    return std::filesystem::equivalent(probe,
                               probe.parent_path() / pathFromQString(variant),
                               equivalentError)
                           && !equivalentError;
                }
            }
            probe = probe.parent_path();
        }
#if defined(_WIN32) || defined(__APPLE__)
        return true;
#else
        return false;
#endif
    }

    std::string pathKey(const std::filesystem::path &path, const bool foldCase)
    {
        const auto normal = path.lexically_normal();
        return foldCase ? pathToQString(normal).toCaseFolded().toStdString() : pathToUtf8(normal);
    }

    std::vector<ScanResult> scanImages(
        const QStringList &inputs, bool recursive, std::vector<ScanIssue> *issues)
    {
        return scanMedia(inputs, recursive, false, issues);
    }

    std::vector<ScanResult> scanMedia(const QStringList &inputs,
        bool recursive,
        const bool includeVideos,
        std::vector<ScanIssue> *issues,
        SkippedTypes *skipped)
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
        std::unordered_map<std::string, bool> directoryFoldsCase;
        const auto markVisited = [&visitedCanonical, &directoryFoldsCase](
                                     const std::filesystem::path &file) -> bool
        {
            std::error_code ec;
            const auto canonical = std::filesystem::canonical(file, ec);
            const auto resolved = ec ? file.lexically_normal() : canonical;
            const auto directory = resolved.parent_path();
            const auto [folds, probed] =
                directoryFoldsCase.try_emplace(pathToUtf8(directory), false);
            if (probed)
            {
                folds->second = namesFoldCase(directory);
            }
            return visitedCanonical.insert(pathKey(resolved, folds->second)).second;
        };

        for (const auto &input : inputs)
        {
            const QFileInfo info(input);
            const auto path = pathFromQString(input);

            if (info.isFile())
            {
                if (markVisited(path))
                {
                    appendFile(results, path, path.parent_path(), includeVideos, skipped);
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
                        appendFile(results, it->path(), path, includeVideos, skipped);
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
