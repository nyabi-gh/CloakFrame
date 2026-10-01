#include "cloakframe/OutputPlan.hpp"

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/VideoIo.hpp"

#include <system_error>
#include <unordered_map>

namespace cloakframe
{
    std::filesystem::path outputRelativePath(const ScanResult &item)
    {
        if (isSupportedVideo(item.sourcePath))
        {
            auto relative = item.relativePath;
            relative.replace_extension(".mp4");
            return relative;
        }
        return item.relativePath;
    }

    std::vector<OutputConflict> findOutputConflicts(const std::vector<ScanResult> &items,
        const std::filesystem::path &outputRoot,
        const std::size_t limit)
    {
        std::vector<OutputConflict> conflicts;
        std::unordered_map<std::string, std::filesystem::path> firstSourceForDestination;
        const bool foldCase = namesFoldCase(outputRoot);

        for (const auto &item : items)
        {
            const auto destination = (outputRoot / outputRelativePath(item)).lexically_normal();
            const auto [it, inserted] =
                firstSourceForDestination.emplace(pathKey(destination, foldCase), item.sourcePath);
            if (!inserted)
            {
                conflicts.push_back({OutputConflict::Kind::DuplicateDestination,
                    item.sourcePath,
                    it->second,
                    destination});
            }

            std::error_code statusError;
            const auto destinationStatus =
                std::filesystem::symlink_status(destination, statusError);
            if (!statusError
                && (std::filesystem::exists(destinationStatus)
                    || std::filesystem::is_symlink(destinationStatus)))
            {
                conflicts.push_back(
                    {OutputConflict::Kind::ExistingDestination, item.sourcePath, {}, destination});
            }

            if (conflicts.size() >= limit)
            {
                break;
            }
        }
        return conflicts;
    }
}
