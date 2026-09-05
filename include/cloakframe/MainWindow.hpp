#pragma once

#include "cloakframe/CustomModelConsent.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/ReviewTypes.hpp"
#include "cloakframe/Theme.hpp"
#include "cloakframe/VideoReviewTypes.hpp"

#include <QByteArray>
#include <QElapsedTimer>
#include <QMainWindow>
#include <QTranslator>
#include <QVector>

#include <functional>
#include <memory>
#include <optional>
#include <vector>

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;
class QShortcut;
class QSpinBox;
class QThread;
class QToolButton;
class QWidget;

namespace cloakframe
{
    struct BuiltinModel;
    class Detector;
    class ProcessorWorker;
    class PlateDetector;
    class SelfUpdater;

    class MainWindow final : public QMainWindow
    {
        Q_OBJECT

    public:
        explicit MainWindow(QWidget *parent = nullptr);

        ~MainWindow() override;

        Q_INVOKABLE cloakframe::ReviewResult requestReview(const QImage &image,
            const QString &sourceName,
            const QVector<QRectF> &detected,
            int currentIndex,
            int total,
            double previewScale);

        Q_INVOKABLE cloakframe::VideoReviewResult requestVideoReview(
            const cloakframe::VideoReviewRequest &request);

    protected:
        void dragEnterEvent(QDragEnterEvent *event) override;

        void dragLeaveEvent(QDragLeaveEvent *event) override;

        void dropEvent(QDropEvent *event) override;

        void closeEvent(QCloseEvent *event) override;

        void changeEvent(QEvent *event) override;

    private slots:
        void chooseModel();

        void chooseCustomImage();

        void downloadSelectedModel();

        void chooseFiles();

        void chooseFolder();

        void chooseOutputDirectory();

        void startProcessing();

        void stopProcessing() const;

        void onWorkerFinished(cloakframe::RunOutcome outcome);

        void toggleAdvanced(bool expanded) const;

        void resetAdvancedDefaults() const;

        void openSettings();

    private:
        void addInputPath(const QString &path) const;

        void populateBundledModels();

        void updateModelPathFromSelection() const;

        [[nodiscard]] QString selectedModelPath() const;

        [[nodiscard]] const BuiltinModel *selectedBuiltinModel() const;

        // True when the file at `path` is the one the user approved, or when they approve it
        // again after being told it changed. Records the new approval in that case.
        [[nodiscard]] bool ensureCustomModelStillApproved(const QString &path);

        void setProcessing(bool processing);

        void setDropHighlight(bool active) const;

        void updateSettingsIcon() const;

        void updateAnonymizationSample() const;

        void updateAnonymizationControls() const;

        [[nodiscard]] QStringList inputPaths() const;

        void appendLog(const QString &message) const;

        void reportValidationIssue(const QString &message, QWidget *field = nullptr) const;

        void loadSettings();

        void saveSettings() const;

        void checkForUpdates();

        void startLegacyUpdateCheck();

        void showUpdateBanner(const QString &latestVersion, const QString &releaseUrl);

        bool askToUpdate(const QString &latestVersion, const QString &releaseNotes);

        void downloadUpdateWithProgress(SelfUpdater *updater, const QString &version);

        void retranslateUi();

        void applyLanguage(const QString &language);

        void addRetranslation(std::function<void()> apply);

        struct DetectorCacheKey
        {
            QString canonicalModelPath;
            qint64 modelSize = -1;
            qint64 modelLastModifiedMs = -1;
            QByteArray modelSha256;
            bool gpuAcceleration = false;
            FaceModelKind faceModelKind = FaceModelKind::Scrfd;

            [[nodiscard]] bool isValid() const
            {
                return !canonicalModelPath.isEmpty() && !modelSha256.isEmpty();
            }

            [[nodiscard]] bool operator==(const DetectorCacheKey &other) const
            {
                return canonicalModelPath == other.canonicalModelPath
                       && modelSize == other.modelSize
                       && modelLastModifiedMs == other.modelLastModifiedMs
                       && modelSha256 == other.modelSha256
                       && gpuAcceleration == other.gpuAcceleration
                       && faceModelKind == other.faceModelKind;
            }
        };

        struct ActiveRunState
        {
            DetectorCacheKey faceKey;
            DetectorCacheKey plateKey;
            AnonymizationMethod method = AnonymizationMethod::Mosaic;
            cv::Mat customImage;
            MaskShape shape = MaskShape::Rectangle;
            int blockSize = 14;
            float padding = 0.18F;
            bool softEdges = false;
            bool preserveMetadata = false;
            bool detectFaces = false;
            bool detectPlates = false;
        };

        [[nodiscard]] static DetectorCacheKey makeDetectorCacheKey(const QString &modelPath,
            bool gpuAcceleration,
            FaceModelKind faceModelKind = FaceModelKind::Scrfd);

        QComboBox *modelCombo_ = nullptr;
        QComboBox *detectCombo_ = nullptr;
        QComboBox *methodCombo_ = nullptr;
        QLabel *customImageLabel_ = nullptr;
        QWidget *customImagePicker_ = nullptr;
        QLineEdit *customImagePathEdit_ = nullptr;
        QPushButton *customImageBrowseButton_ = nullptr;
        QComboBox *shapeCombo_ = nullptr;
        QCheckBox *softEdgeCheck_ = nullptr;
        QToolButton *settingsButton_ = nullptr;
        QShortcut *settingsShortcut_ = nullptr;
        QLineEdit *modelPathEdit_ = nullptr;
        QPushButton *downloadButton_ = nullptr;
        QPushButton *modelBrowseButton_ = nullptr;
        QLineEdit *outputDirEdit_ = nullptr;
        QPushButton *outputBrowseButton_ = nullptr;
        QListWidget *inputList_ = nullptr;
        QPushButton *addFilesButton_ = nullptr;
        QPushButton *addFolderButton_ = nullptr;
        QPushButton *clearInputsButton_ = nullptr;
        QCheckBox *recursiveCheck_ = nullptr;
        QCheckBox *reviewCheck_ = nullptr;
        QCheckBox *preserveMetaCheck_ = nullptr;
        QLabel *updateLabel_ = nullptr;
        QDoubleSpinBox *scoreThresholdSpin_ = nullptr;
        QDoubleSpinBox *nmsThresholdSpin_ = nullptr;
        QSpinBox *blockSizeSpin_ = nullptr;
        QDoubleSpinBox *paddingSpin_ = nullptr;
        QProgressBar *progressBar_ = nullptr;
        QPlainTextEdit *logEdit_ = nullptr;
        QPushButton *startButton_ = nullptr;
        QPushButton *stopButton_ = nullptr;
        QPushButton *openOutputButton_ = nullptr;
        QPushButton *resultsButton_ = nullptr;
        QLabel *statusLabel_ = nullptr;
        QLabel *samplePreview_ = nullptr;
        QToolButton *advancedToggle_ = nullptr;
        QPushButton *resetAdvancedButton_ = nullptr;
        QWidget *advancedBody_ = nullptr;

        QThread *workerThread_ = nullptr;
        ProcessorWorker *worker_ = nullptr;
        QElapsedTimer runTimer_;

        std::shared_ptr<Detector> cachedDetector_;
        std::shared_ptr<Detector> cachedVideoDetector_;
        DetectorCacheKey cachedDetectorKey_;
        DetectorCacheKey cachedVideoDetectorKey_;
        std::shared_ptr<PlateDetector> cachedPlateDetector_;
        DetectorCacheKey cachedPlateDetectorKey_;
        std::optional<ActiveRunState> activeRunState_;
        cv::Mat customImage_;
        CustomModelApproval customModelApproval_;

        QTranslator translator_;
        QTranslator qtTranslator_;
        QString language_;
        ThemeMode themeMode_ = ThemeMode::System;
        bool checkForUpdatesOnStartup_ = true;
        bool fileLogging_ = true;
        bool gpuAcceleration_ = true;
        int videoQuality_ = 0;
        int videoCodec_ = 0;
        bool processing_ = false;
        bool shuttingDown_ = false;
        RunSummary lastRunSummary_;
        QVector<FileResult> fileResults_;
        std::vector<std::function<void()>> retranslators_;
    };
}
