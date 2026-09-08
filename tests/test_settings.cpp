#include "cloakframe/SettingsDialog.hpp"

#include <QApplication>
#include <QCheckBox>
#include <QTranslator>

#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication app(argc, argv);
    QTranslator translator;
    const QString translation = qEnvironmentVariable("CLOAKFRAME_TEST_TRANSLATION");
    if (!translation.isEmpty())
    {
        assert(translator.load(translation));
        app.installTranslator(&translator);
    }
    cloakframe::SettingsDialog dialog(cloakframe::ThemeMode::System, "ko", true, false, true, 0, 0);
    int changes = 0;
    QObject::connect(&dialog,
        &cloakframe::SettingsDialog::fileLoggingChanged,
        &dialog,
        [&](bool)
        {
            ++changes;
        });
    auto *logging = dialog.findChild<QCheckBox *>("detailedLogging");
    assert(logging && !logging->isChecked());
    logging->setChecked(true);
    logging->setChecked(false);
    assert(changes == 2);
    const QString screenshot = qEnvironmentVariable("CLOAKFRAME_TEST_SCREENSHOT");
    if (!screenshot.isEmpty())
    {
        dialog.show();
        app.processEvents();
        assert(dialog.grab().save(screenshot));
    }
    return 0;
}
