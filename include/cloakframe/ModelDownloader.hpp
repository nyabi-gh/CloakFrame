#pragma once

#include <QString>

class QWidget;

namespace cloakframe
{
    struct BuiltinModel;

    bool downloadModelWithProgress(
        QWidget *parent, const BuiltinModel &model, const QString &destPath);
    bool ensureBuiltinModelAvailable(
        QWidget *parent, const BuiltinModel &model, const QString &destPath);
    bool ensurePlateModelAvailable(QWidget *parent, const QString &destPath);
    bool customModelFileIsAllowed(QWidget *parent, const QString &path);
    bool confirmTrustedCustomModel(QWidget *parent, const QString &path);

    // Asked when the bytes at an already approved path are not the ones that were approved.
    // Separate from the prompt above because the answer means something different: not "do you
    // trust where this came from" but "did you mean to replace it".
    bool confirmChangedCustomModel(QWidget *parent, const QString &path);
}
