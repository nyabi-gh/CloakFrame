#include "cloakframe/CustomModelConsent.hpp"

namespace cloakframe
{
    bool CustomModelApproval::isRecorded() const
    {
        return !digest.isEmpty() && size > 0;
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
