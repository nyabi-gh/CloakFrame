#include "cloakframe/CustomModelConsent.hpp"

#include "cloakframe/UpdateSignature.hpp"

#include <QFileInfo>

namespace cloakframe
{
    bool CustomModelApproval::isRecorded() const
    {
        return !digest.isEmpty() && size > 0;
    }

    std::optional<CustomModelApproval> approvalForCustomModel(const QString &path)
    {
        const QFileInfo info(path);
        if (!info.exists() || !info.isFile())
        {
            return std::nullopt;
        }
        const auto digest = sha256HexOfFile(path);
        if (!digest)
        {
            return std::nullopt;
        }
        CustomModelApproval approval;
        approval.digest = QString::fromLatin1(*digest).toLower();
        approval.size = info.size();
        if (!approval.isRecorded())
        {
            return std::nullopt;
        }
        return approval;
    }

    CustomModelState checkCustomModel(const QString &path, const CustomModelApproval &approved)
    {
        const QFileInfo info(path);
        if (!info.exists() || !info.isFile())
        {
            return CustomModelState::Unavailable;
        }
        if (!approved.isRecorded())
        {
            return CustomModelState::Unapproved;
        }
        // Size first: it settles most replacements without reading up to 512 MB, and a file
        // whose size matches still has to be hashed anyway.
        if (info.size() != approved.size)
        {
            return CustomModelState::Unapproved;
        }
        const auto digest = sha256HexOfFile(path);
        if (!digest)
        {
            return CustomModelState::Unavailable;
        }
        return QString::fromLatin1(*digest).toLower() == approved.digest.toLower()
                   ? CustomModelState::Approved
                   : CustomModelState::Unapproved;
    }
}
