#pragma once

#include <QByteArray>
#include <QString>

namespace cloakframe
{
    // A custom ONNX model is parsed and executed by the native runtime inside this process, so
    // which bytes the user agreed to load is the whole question. Approval used to be remembered
    // as a path, and a path says nothing about what is at it the next time it is read: a synced
    // folder, an editor, or a removable volume can replace the file between one run and the
    // next, and the replacement would load without anyone being asked.
    //
    // What is recorded is therefore the content, and every load compares against it.

    struct CustomModelApproval
    {
        QString digest;
        qint64 size = 0;

        // False for a model chosen before approvals were recorded as content, which has to be
        // treated as never approved rather than as approved for anything.
        [[nodiscard]] bool isRecorded() const;

        bool operator==(const CustomModelApproval &) const = default;
    };

    // What to record once the user approves bytes that `digestModelFile` hashed. `sha256` is the
    // raw 32-byte digest.
    [[nodiscard]] CustomModelApproval approvalForDigest(const QByteArray &sha256, qint64 size);

    // Whether `approved` covers bytes with this digest and size. A run compares the digest it
    // hands the detector, which loads only bytes that hash to it, so what was approved and
    // what is loaded cannot differ.
    [[nodiscard]] bool approvalCovers(
        const CustomModelApproval &approved, const QByteArray &sha256, qint64 size);
}
