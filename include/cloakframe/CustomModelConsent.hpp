#pragma once

#include <QString>

#include <optional>

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

    enum class CustomModelState
    {
        // The file at the path is the one that was approved.
        Approved,
        // Something readable is there, but it is not what was approved - or nothing was ever
        // approved. Both mean the user has to be asked before it is loaded.
        Unapproved,
        // Nothing readable is at the path, so there is nothing to ask about yet.
        Unavailable,
    };

    // What to record once the user approves the file at `path`, or nothing if it cannot be read.
    [[nodiscard]] std::optional<CustomModelApproval> approvalForCustomModel(const QString &path);

    [[nodiscard]] CustomModelState checkCustomModel(
        const QString &path, const CustomModelApproval &approved);
}
