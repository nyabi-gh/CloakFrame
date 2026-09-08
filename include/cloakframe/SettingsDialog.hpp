#pragma once

#include "cloakframe/Theme.hpp"

#include <QDialog>
#include <QString>

class QCheckBox;
class QComboBox;
class QLabel;
class QPushButton;

namespace cloakframe
{
    class SettingsDialog final : public QDialog
    {
        Q_OBJECT

    public:
        SettingsDialog(ThemeMode theme,
            const QString &language,
            bool checkForUpdates,
            bool fileLogging,
            bool gpuAcceleration,
            int videoQuality,
            int videoCodec,
            QWidget *parent = nullptr);

    signals:
        void themeChanged(cloakframe::ThemeMode mode);

        void languageChanged(const QString &language);

        void checkForUpdatesChanged(bool enabled);

        void fileLoggingChanged(bool enabled);

        void gpuAccelerationChanged(bool enabled);

        void videoQualityChanged(int quality);

        void videoCodecChanged(int codec);

    protected:
        void changeEvent(QEvent *event) override;

    private:
        void retranslate();

        QLabel *themeLabel_ = nullptr;
        QLabel *languageLabel_ = nullptr;
        QLabel *videoQualityLabel_ = nullptr;
        QLabel *videoCodecLabel_ = nullptr;
        QComboBox *themeCombo_ = nullptr;
        QComboBox *languageCombo_ = nullptr;
        QComboBox *videoQualityCombo_ = nullptr;
        QComboBox *videoCodecCombo_ = nullptr;
        QCheckBox *updateCheck_ = nullptr;
        QCheckBox *logCheck_ = nullptr;
        QCheckBox *gpuCheck_ = nullptr;
        QPushButton *openLogsButton_ = nullptr;
        QPushButton *clearLogsButton_ = nullptr;
        QPushButton *closeButton_ = nullptr;
    };
}
