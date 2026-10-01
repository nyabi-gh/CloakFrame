#include "cloakframe/ReviewDialog.hpp"

#include <QApplication>
#include <QKeyEvent>
#include <QShortcut>

#include <algorithm>
#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    QImage image(320, 240, QImage::Format_RGB32);
    image.fill(Qt::gray);
    cloakframe::ReviewDialog dialog(
        image, "photo.jpg", {QRectF(20, 20, 40, 40)}, 1, 2, false, {}, nullptr);
    dialog.show();
    dialog.activateWindow();
    application.processEvents();

    const auto widgets = dialog.findChildren<QWidget *>();
    const auto canvas = std::find_if(widgets.cbegin(),
        widgets.cend(),
        [](const QWidget *widget)
        {
            return widget->accessibleName() == "Review image";
        });
    assert(canvas != widgets.cend());
    for (QWidget *target : {static_cast<QWidget *>(&dialog), *canvas})
    {
        for (const auto key : {Qt::Key_Return, Qt::Key_Enter})
        {
            target->setFocus();
            application.processEvents();
            QKeyEvent press(QEvent::KeyPress, key, Qt::NoModifier);
            QApplication::sendEvent(target, &press);
            application.processEvents();
            assert(dialog.isVisible() && dialog.result() == QDialog::Rejected);
        }
    }
    assert(dialog.reviewResult().finalBoxes.size() == 1);

    const auto shortcuts = dialog.findChildren<QShortcut *>();
    const auto saveShortcut = std::find_if(shortcuts.cbegin(),
        shortcuts.cend(),
        [](const QShortcut *shortcut)
        {
            return shortcut->key() == QKeySequence(Qt::CTRL | Qt::Key_Return);
        });
    assert(saveShortcut != shortcuts.cend());
    emit(*saveShortcut)->activated();
    assert(!dialog.isVisible() && dialog.result() == QDialog::Accepted);
    assert(dialog.reviewResult().decision == cloakframe::ReviewDecision::Save);
    assert(dialog.reviewResult().finalBoxes.size() == 1);
    return 0;
}
