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

    CustomModelApproval approvalForDigest(const QByteArray &sha256, const qint64 size)
    {
        CustomModelApproval approval;
        if (sha256.size() == 32)
        {
            approval.digest = QString::fromLatin1(sha256.toHex());
            approval.size = size;
        }
        return approval;
    }

    bool approvalCovers(
        const CustomModelApproval &approved, const QByteArray &sha256, const qint64 size)
    {
        return approved.isRecorded() && sha256.size() == 32 && size == approved.size
               && QString::fromLatin1(sha256.toHex()) == approved.digest.toLower();
    }
}
