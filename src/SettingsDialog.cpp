#include "cloakframe/SettingsDialog.hpp"

#include "cloakframe/Logging.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDesktopServices>
#include <QDialogButtonBox>
#include <QEvent>
#include <QFormLayout>
#include <QLabel>
#include <QMessageBox>
#include <QPushButton>
#include <QUrl>
#include <QVBoxLayout>

namespace cloakframe
{
    SettingsDialog::SettingsDialog(ThemeMode theme,
        const QString &language,
        bool checkForUpdates,
        bool fileLogging,
        bool gpuAcceleration,
        int videoQuality,
        int videoCodec,
        QWidget *parent)
        : QDialog(parent)
    {
        setModal(true);

        auto *root = new QVBoxLayout(this);
        root->setContentsMargins(20, 20, 20, 20);
        root->setSpacing(16);

        auto *form = new QFormLayout();
        form->setLabelAlignment(Qt::AlignLeft);
        form->setFormAlignment(Qt::AlignLeft | Qt::AlignTop);
        form->setHorizontalSpacing(18);
        form->setVerticalSpacing(12);

        themeCombo_ = new QComboBox(this);
        themeCombo_->addItem(QString(), static_cast<int>(ThemeMode::System));
        themeCombo_->addItem(QString(), static_cast<int>(ThemeMode::Light));
        themeCombo_->addItem(QString(), static_cast<int>(ThemeMode::Dark));
        const int themeIndex = themeCombo_->findData(static_cast<int>(theme));
        themeCombo_->setCurrentIndex(themeIndex >= 0 ? themeIndex : 0);

        languageCombo_ = new QComboBox(this);
        languageCombo_->addItem("English", "en");
        languageCombo_->addItem("한국어", "ko");
        languageCombo_->addItem("日本語", "ja");
        languageCombo_->addItem("简体中文", "zh_CN");
        const int languageIndex = languageCombo_->findData(language);
        languageCombo_->setCurrentIndex(languageIndex >= 0 ? languageIndex : 0);

        videoQualityCombo_ = new QComboBox(this);
        videoQualityCombo_->addItem(QString(), 0);
        videoQualityCombo_->addItem(QString(), 1);
        videoQualityCombo_->addItem(QString(), 2);
        const int qualityIndex = videoQualityCombo_->findData(videoQuality);
        videoQualityCombo_->setCurrentIndex(qualityIndex >= 0 ? qualityIndex : 0);

        videoCodecCombo_ = new QComboBox(this);
        videoCodecCombo_->addItem(QString(), 0);
        videoCodecCombo_->addItem(QString(), 1);
        const int codecIndex = videoCodecCombo_->findData(videoCodec);
        videoCodecCombo_->setCurrentIndex(codecIndex >= 0 ? codecIndex : 0);

        themeLabel_ = new QLabel(this);
        languageLabel_ = new QLabel(this);
        videoQualityLabel_ = new QLabel(this);
        videoCodecLabel_ = new QLabel(this);
        form->addRow(themeLabel_, themeCombo_);
        form->addRow(languageLabel_, languageCombo_);
        form->addRow(videoQualityLabel_, videoQualityCombo_);
        form->addRow(videoCodecLabel_, videoCodecCombo_);
        root->addLayout(form);

        updateCheck_ = new QCheckBox(this);
        updateCheck_->setChecked(checkForUpdates);
        root->addWidget(updateCheck_);

        logCheck_ = new QCheckBox(this);
        logCheck_->setObjectName("detailedLogging");
        logCheck_->setChecked(fileLogging);
        root->addWidget(logCheck_);

        openLogsButton_ = new QPushButton(this);
        clearLogsButton_ = new QPushButton(this);
        root->addWidget(openLogsButton_);
        root->addWidget(clearLogsButton_);
        connect(openLogsButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                if (!QDesktopServices::openUrl(QUrl::fromLocalFile(localLogDirectory())))
                    QMessageBox::warning(
                        this, tr("Local logs"), tr("Could not open the log folder."));
            });
        connect(clearLogsButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                if (!clearLocalLogs())
                    QMessageBox::warning(
                        this, tr("Local logs"), tr("Could not delete all local logs."));
            });

        gpuCheck_ = new QCheckBox(this);
        gpuCheck_->setChecked(gpuAcceleration);
        root->addWidget(gpuCheck_);

        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
        closeButton_ = buttons->button(QDialogButtonBox::Close);
        closeButton_->setObjectName("primaryButton");
        closeButton_->setCursor(Qt::PointingHandCursor);
        root->addWidget(buttons);

        connect(themeCombo_,
            &QComboBox::currentIndexChanged,
            this,
            [this]
            {
                emit themeChanged(static_cast<ThemeMode>(themeCombo_->currentData().toInt()));
            });
        connect(languageCombo_,
            &QComboBox::currentIndexChanged,
            this,
            [this]
            {
                emit languageChanged(languageCombo_->currentData().toString());
            });
        connect(updateCheck_,
            &QCheckBox::toggled,
            this,
            [this](bool enabled)
            {
                emit checkForUpdatesChanged(enabled);
            });
        connect(logCheck_,
            &QCheckBox::toggled,
            this,
            [this](bool enabled)
            {
                emit fileLoggingChanged(enabled);
            });
        connect(gpuCheck_,
            &QCheckBox::toggled,
            this,
            [this](bool enabled)
            {
                emit gpuAccelerationChanged(enabled);
            });
        connect(videoQualityCombo_,
            &QComboBox::currentIndexChanged,
            this,
            [this]
            {
                emit videoQualityChanged(videoQualityCombo_->currentData().toInt());
            });
        connect(videoCodecCombo_,
            &QComboBox::currentIndexChanged,
            this,
            [this]
            {
                emit videoCodecChanged(videoCodecCombo_->currentData().toInt());
            });
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);

        retranslate();
    }

    void SettingsDialog::retranslate()
    {
        setWindowTitle(tr("Settings"));
        themeLabel_->setText(tr("Theme"));
        languageLabel_->setText(tr("Language"));
        themeCombo_->setItemText(0, tr("System"));
        themeCombo_->setItemText(1, tr("Light"));
        themeCombo_->setItemText(2, tr("Dark"));
        updateCheck_->setText(tr("Check for updates on startup"));
        logCheck_->setText(tr("Include file names in detailed local logs"));
        logCheck_->setToolTip(
            tr("Off by default. Basic diagnostics contain no file names. "
               "Detailed logs may contain file names and paths. Stored on this device only. "
               "Changes apply immediately; existing logs remain until deleted."));
        openLogsButton_->setText(tr("Open log folder"));
        clearLogsButton_->setText(tr("Delete local logs"));
        gpuCheck_->setText(tr("Use GPU acceleration"));
        gpuCheck_->setToolTip(tr("Runs detection models and video encoding on the GPU "
                                 "when available. Applies from the next run."));
        videoQualityLabel_->setText(tr("Video quality"));
        videoQualityCombo_->setItemText(0, tr("High (near-original)"));
        videoQualityCombo_->setItemText(1, tr("Balanced"));
        videoQualityCombo_->setItemText(2, tr("Smaller files"));
        videoQualityCombo_->setToolTip(tr("Quality of re-encoded videos. "
                                          "Higher quality produces larger files."));
        videoCodecLabel_->setText(tr("Video codec"));
        videoCodecCombo_->setItemText(0, tr("H.264 (most compatible)"));
        videoCodecCombo_->setItemText(1, tr("HEVC (smaller files)"));
        videoCodecCombo_->setToolTip(tr("Codec for re-encoded videos. HEVC produces "
                                        "smaller files but may not play on older devices."));
        if (closeButton_ != nullptr)
        {
            closeButton_->setText(tr("Close"));
        }
    }

    void SettingsDialog::changeEvent(QEvent *event)
    {
        if (event->type() == QEvent::LanguageChange)
        {
            retranslate();
        }
        QDialog::changeEvent(event);
    }
}
