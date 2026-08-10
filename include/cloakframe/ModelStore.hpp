#pragma once

#include <QByteArray>
#include <QString>

namespace cloakframe
{
    // A downloaded model has already been checked against the digest the catalog pins by the
    // time it gets here. Putting it on disk is the part that can still go wrong.
    //
    // The old path wrote to `<model>.part`, a name anyone who can reach the folder could
    // predict and replace with a link to somewhere else, and it deleted the model already in
    // place before the new one had a name. Both are fixed here: the temporary file has an
    // unpredictable name and is created exclusively, and the model in place is only ever
    // replaced by a file that has been read back and found to be the one that was verified.

    enum class ModelSaveResult
    {
        // The verified file was published, or another writer already published the same digest.
        Saved,
        // Another account owns the model folder, or someone outside this account can write to
        // it. Publishing there would hand verified bytes to whoever controls the folder.
        FolderNotPrivate,
        // The temporary file could not be created, written, or flushed.
        WriteFailed,
        // What reached the disk is not what was verified in memory.
        ContentMismatch,
        // The finished file could not take the model's name. Whatever was there is untouched.
        PublishFailed,
    };

    // Writes `bytes` through a private temporary file in the model's own folder, reads it back
    // through that same descriptor to confirm `expectedSha256Hex`, and only then gives that open
    // file the model's name. A concurrent writer that publishes the same digest also satisfies
    // the operation. `expectedSha256Hex` is compared case-insensitively.
    [[nodiscard]] ModelSaveResult saveModelFile(
        const QString &destPath, const QByteArray &bytes, const QString &expectedSha256Hex);
}
