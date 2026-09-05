#include "cloakframe/MainWindow.hpp"

#include "cloakframe/Detector.hpp"
#include "cloakframe/ImageIo.hpp"
#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/ModelCatalog.hpp"
#include "cloakframe/ModelDownloader.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/PathUtil.hpp"
#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/ReleaseNotes.hpp"
#include "cloakframe/ReviewDialog.hpp"
#include "cloakframe/SelfUpdater.hpp"
#include "cloakframe/SettingsDialog.hpp"
#include "cloakframe/Theme.hpp"
#include "cloakframe/UpdateChecker.hpp"
#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoReviewDialog.hpp"

#include <QApplication>
#include <QCheckBox>
#include <QCloseEvent>
#include <QComboBox>
#include <QContextMenuEvent>
#include <QCryptographicHash>
#include <QDateTime>
#include <QDesktopServices>
#include <QDir>
#include <QEvent>
#include <QEventLoop>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QFrame>
#include <QHBoxLayout>
#include <QImageReader>
#include <QKeyEvent>
#include <QLabel>
#include <QLibraryInfo>
#include <QLineEdit>
#include <QListWidget>
#include <QLocale>
#include <QMenu>
#include <QMessageBox>
#include <QMimeData>
#include <QPaintEvent>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPointer>
#include <QProcess>
#include <QProgressBar>
#include <QProgressDialog>
#include <QPushButton>
#include <QScreen>
#include <QScrollArea>
#include <QSettings>
#include <QShortcut>
#include <QSpinBox>
#include <QStandardPaths>
#include <QStatusBar>
#include <QStyle>
#include <QStyleHints>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <QUrl>
#include <QWidget>

#include <opencv2/imgproc.hpp>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <cmath>

namespace cloakframe
{
    namespace
    {
        constexpr double kDefaultScoreThreshold = 0.5;
        constexpr double kDefaultNmsThreshold = 0.4;
        constexpr int kDefaultBlockSize = 14;
        constexpr double kDefaultPadding = 0.18;
        constexpr int kModelCatalogIndexRole = Qt::UserRole + 1;
        constexpr qint64 kMaxCustomImageBytes = 64LL * 1024LL * 1024LL;
        constexpr int kMaxCustomImageLongEdge = 2048;

        struct CustomImageLoadResult
        {
            cv::Mat image;
            QString canonicalPath;
            QString error;
        };

        CustomImageLoadResult loadCustomImage(const QString &path)
        {
            const QFileInfo info(path);
            if (!info.exists() || !info.isFile())
            {
                return {{},
                    {},
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "Choose an existing image file.")};
            }
            QFile file(info.absoluteFilePath());
            if (!file.open(QIODevice::ReadOnly))
            {
                return {{},
                    {},
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "Choose an existing image file.")};
            }
            if (file.size() <= 0 || file.size() > kMaxCustomImageBytes)
            {
                return {{},
                    {},
                    QCoreApplication::translate("cloakframe::MainWindow",
                        "The selected image must be no larger than 64 MB.")};
            }

            QImageReader reader(&file);
            reader.setAutoTransform(true);
            if (!reader.canRead())
            {
                return {{},
                    {},
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "The selected file is not a supported image.")};
            }

            const QSize sourceSize = reader.size();
            if (!sourceSize.isValid() || sourceSize.width() <= 0 || sourceSize.height() <= 0)
            {
                return {{},
                    {},
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "The selected image has invalid dimensions.")};
            }

            QSize decodedSize = sourceSize;
            if (std::max(decodedSize.width(), decodedSize.height()) > kMaxCustomImageLongEdge)
            {
                decodedSize.scale(
                    kMaxCustomImageLongEdge, kMaxCustomImageLongEdge, Qt::KeepAspectRatio);
                reader.setScaledSize(decodedSize);
            }

            QImage decoded = reader.read();
            if (decoded.isNull())
            {
                return {{},
                    {},
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "The selected image could not be decoded: %1")
                        .arg(reader.errorString())};
            }
            decoded = decoded.convertToFormat(QImage::Format_RGBA8888);
            cv::Mat rgba(decoded.height(),
                decoded.width(),
                CV_8UC4,
                decoded.bits(),
                static_cast<std::size_t>(decoded.bytesPerLine()));
            cv::Mat bgra;
            cv::cvtColor(rgba, bgra, cv::COLOR_RGBA2BGRA);

            QString canonicalPath = info.canonicalFilePath();
            if (canonicalPath.isEmpty())
            {
                canonicalPath = QDir::cleanPath(info.absoluteFilePath());
            }
            return {std::move(bgra), std::move(canonicalPath), {}};
        }

        QString releaseNotesSection(const QString &releaseNotes)
        {
            // Notes carry every language the release ships with, so the dialog shows the one
            // matching the interface rather than all of them stacked together.
            return releaseNotesForLanguage(releaseNotes,
                QLocale().language() == QLocale::C ? QStringLiteral("en")
                                                   : QLocale().name().left(2));
        }

        QString defaultOutputDirectory()
        {
            const auto pictures =
                QStandardPaths::writableLocation(QStandardPaths::PicturesLocation);
            if (!pictures.isEmpty())
            {
                return pictures + "/CloakFrame";
            }
            return QDir::homePath() + "/CloakFrame";
        }

        QLabel *makeSectionTitle(QWidget *parent)
        {
            auto *label = new QLabel(parent);
            label->setProperty("role", "sectionTitle");
            return label;
        }

        QLabel *makeSectionHint(QWidget *parent)
        {
            auto *label = new QLabel(parent);
            label->setProperty("role", "sectionHint");
            label->setWordWrap(true);
            return label;
        }

        QLabel *makeFieldLabel(QWidget *parent)
        {
            auto *label = new QLabel(parent);
            label->setProperty("role", "fieldLabel");
            return label;
        }

        QIcon videoThumbnailIcon(const QString &path)
        {
            const auto tools = locateFfmpegTools();
            if (!tools)
            {
                return {};
            }
            QProcess process;
            process.start(tools->ffmpegPath,
                {"-v",
                    "error",
                    "-ss",
                    "0",
                    "-i",
                    path,
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=80:-2",
                    "-f",
                    "image2pipe",
                    "-c:v",
                    "png",
                    "-"});
            if (!process.waitForStarted(3000) || !process.waitForFinished(3000))
            {
                process.kill();
                return {};
            }
            if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
            {
                return {};
            }
            QImage image;
            if (!image.loadFromData(process.readAllStandardOutput(), "PNG") || image.isNull())
            {
                return {};
            }
            return QIcon(QPixmap::fromImage(image));
        }

        QFrame *makeCard(QWidget *parent)
        {
            auto *card = new QFrame(parent);
            card->setObjectName("card");
            card->setFrameShape(QFrame::NoFrame);
            return card;
        }

        QPixmap anonymizationSamplePixmap(AnonymizationMethod method,
            MaskShape shape,
            int blockSize,
            float padding,
            bool softEdges,
            const cv::Mat &customImage)
        {
            qreal dpr = 1.0;
            for (const QScreen *screen : QGuiApplication::screens())
            {
                dpr = std::max(dpr, screen->devicePixelRatio());
            }
            const int w = static_cast<int>(std::lround(152 * dpr));
            const int h = static_cast<int>(std::lround(96 * dpr));

            cv::Mat sample(h, w, CV_8UC3);
            const int cell = std::max(2, static_cast<int>(std::lround(6 * dpr)));
            for (int y = 0; y < h; ++y)
            {
                for (int x = 0; x < w; ++x)
                {
                    const bool checker = ((x / cell) + (y / cell)) % 2 == 0;
                    const int base = checker ? 205 : 150;
                    sample.at<cv::Vec3b>(y, x) =
                        cv::Vec3b(static_cast<uchar>(std::clamp(base - (y * 60) / h, 0, 255)),
                            static_cast<uchar>(std::clamp(base - 40 + (x * 70) / w, 0, 255)),
                            static_cast<uchar>(std::clamp(base + (y * 40) / h - 20, 0, 255)));
                }
            }

            FaceDetections detections;
            const auto sampleWidth = static_cast<float>(w);
            const auto sampleHeight = static_cast<float>(h);
            detections.push_back({cv::Rect2f(sampleWidth * 0.3F,
                                      sampleHeight * 0.2F,
                                      sampleWidth * 0.4F,
                                      sampleHeight * 0.6F),
                1.0F});
            applyAnonymization(sample,
                detections,
                method,
                std::max(2, static_cast<int>(std::lround(blockSize * dpr))),
                padding,
                shape,
                softEdges,
                customImage);

            const QImage image(sample.data,
                sample.cols,
                sample.rows,
                static_cast<int>(sample.step),
                QImage::Format_BGR888);
            QPixmap pixmap = QPixmap::fromImage(image.copy());
            pixmap.setDevicePixelRatio(dpr);
            return pixmap;
        }

        class DropListWidget final : public QListWidget
        {
        public:
            using QListWidget::QListWidget;

            void setPlaceholderText(const QString &text)
            {
                placeholder_ = text;
                viewport()->update();
            }

            void removeSelectedItems()
            {
                const auto items = selectedItems();
                for (auto *item : items)
                {
                    delete item;
                }
            }

        protected:
            void paintEvent(QPaintEvent *event) override
            {
                QListWidget::paintEvent(event);
                if (count() != 0 || placeholder_.isEmpty())
                {
                    return;
                }
                QPainter painter(viewport());
                painter.setPen(palette().placeholderText().color());
                painter.drawText(viewport()->rect().adjusted(24, 0, -24, 0),
                    Qt::AlignCenter | Qt::TextWordWrap,
                    placeholder_);
            }

            void keyPressEvent(QKeyEvent *event) override
            {
                if (event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace)
                {
                    removeSelectedItems();
                    return;
                }
                QListWidget::keyPressEvent(event);
            }

            void contextMenuEvent(QContextMenuEvent *event) override
            {
                QMenu menu(this);
                QAction *removeAction = menu.addAction(
                    QCoreApplication::translate("cloakframe::MainWindow", "Remove Selected"));
                removeAction->setEnabled(!selectedItems().isEmpty());
                QAction *clearAction = menu.addAction(
                    QCoreApplication::translate("cloakframe::MainWindow", "Clear All"));
                clearAction->setEnabled(count() > 0);
                QAction *chosen = menu.exec(event->globalPos());
                if (chosen == removeAction)
                {
                    removeSelectedItems();
                }
                else if (chosen == clearAction)
                {
                    clear();
                }
            }

        private:
            QString placeholder_;
        };
    }

    MainWindow::MainWindow(QWidget *parent)
        : QMainWindow(parent)
    {
        qRegisterMetaType<VideoReviewRequest>();
        qRegisterMetaType<VideoReviewResult>();
        setWindowTitle("CloakFrame");
        setAcceptDrops(true);
        resize(920, 760);
        setMinimumSize(720, 600);

        auto *container = new QWidget(this);
        auto *containerLayout = new QVBoxLayout(container);
        containerLayout->setContentsMargins(0, 0, 0, 0);
        containerLayout->setSpacing(0);

        auto *scrollArea = new QScrollArea(container);
        scrollArea->setObjectName("rootScroll");
        scrollArea->setWidgetResizable(true);
        scrollArea->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        scrollArea->setFrameShape(QFrame::NoFrame);

        auto *central = new QWidget(scrollArea);
        auto *root = new QVBoxLayout(central);
        root->setContentsMargins(32, 28, 32, 28);
        root->setSpacing(20);

        auto *header = new QWidget(central);
        auto *headerLayout = new QVBoxLayout(header);
        headerLayout->setContentsMargins(0, 0, 0, 0);
        headerLayout->setSpacing(4);

        auto *titleRow = new QHBoxLayout();
        titleRow->setContentsMargins(0, 0, 0, 0);
        titleRow->setSpacing(8);
        auto *title = new QLabel("CloakFrame", header);
        title->setObjectName("titleLabel");

        settingsButton_ = new QToolButton(header);
        settingsButton_->setObjectName("settingsButton");
        settingsButton_->setFixedSize(36, 36);
        settingsButton_->setCursor(Qt::PointingHandCursor);
        updateSettingsIcon();
        addRetranslation(
            [this]
            {
                settingsButton_->setToolTip(tr("Settings"));
                settingsButton_->setAccessibleName(tr("Settings"));
            });
        connect(settingsButton_, &QToolButton::clicked, this, &MainWindow::openSettings);
        settingsShortcut_ = new QShortcut(QKeySequence::Preferences, this);
        connect(settingsShortcut_, &QShortcut::activated, this, &MainWindow::openSettings);

        auto *versionLabel =
            new QLabel(QString("v%1").arg(QCoreApplication::applicationVersion()), header);
        versionLabel->setObjectName("subtitleLabel");

        updateLabel_ = new QLabel(header);
        updateLabel_->setObjectName("subtitleLabel");
        updateLabel_->setTextFormat(Qt::RichText);
        updateLabel_->setOpenExternalLinks(true);
        updateLabel_->setVisible(false);

        auto *titleGroup = new QWidget(header);
        auto *titleGroupLayout = new QHBoxLayout(titleGroup);
        titleGroupLayout->setContentsMargins(0, 0, 0, 0);
        titleGroupLayout->setSpacing(8);
        titleGroupLayout->addWidget(title, 0, Qt::AlignBottom);
        titleGroupLayout->addWidget(versionLabel, 0, Qt::AlignBottom);
        titleGroupLayout->addWidget(updateLabel_, 0, Qt::AlignBottom);

        titleRow->addWidget(titleGroup, 0, Qt::AlignVCenter);
        titleRow->addStretch(1);
        titleRow->addWidget(settingsButton_, 0, Qt::AlignVCenter);

        auto *subtitle = new QLabel(header);
        subtitle->setObjectName("subtitleLabel");
        addRetranslation(
            [subtitle]
            {
                subtitle->setText(tr(
                    "Local, private redaction of faces and license plates in photos and videos"));
            });
        headerLayout->addLayout(titleRow);
        headerLayout->addWidget(subtitle);
        root->addWidget(header);

        {
            auto *card = makeCard(central);
            auto *cardLayout = new QVBoxLayout(card);
            cardLayout->setContentsMargins(20, 18, 20, 18);
            cardLayout->setSpacing(12);

            auto *modelTitle = makeSectionTitle(card);
            cardLayout->addWidget(modelTitle);
            addRetranslation(
                [modelTitle]
                {
                    modelTitle->setText(tr("Model"));
                });

            auto *modelHint = makeSectionHint(card);
            cardLayout->addWidget(modelHint);
            addRetranslation(
                [modelHint]
                {
                    modelHint->setText(
                        tr("Choose speed vs. accuracy, or load a custom SCRFD ONNX file."));
                });

            detectCombo_ = new QComboBox(card);
            detectCombo_->setMinimumHeight(34);
            detectCombo_->addItem(QString(), "faces");
            detectCombo_->addItem(QString(), "plates");
            detectCombo_->addItem(QString(), "both");
            addRetranslation(
                [this]
                {
                    detectCombo_->setItemText(0, tr("Faces"));
                    detectCombo_->setItemText(1, tr("License plates"));
                    detectCombo_->setItemText(2, tr("Faces + license plates"));
                });
            cardLayout->addWidget(detectCombo_);

            modelCombo_ = new QComboBox(card);
            modelCombo_->setMinimumHeight(34);
            cardLayout->addWidget(modelCombo_);
            connect(detectCombo_,
                &QComboBox::currentIndexChanged,
                this,
                [this]
                {
                    const bool facesNeeded = detectCombo_->currentData().toString() != "plates";
                    modelCombo_->setEnabled(!processing_ && facesNeeded);
                    modelPathEdit_->setEnabled(!processing_ && facesNeeded);
                    modelBrowseButton_->setEnabled(!processing_ && facesNeeded);
                    downloadButton_->setEnabled(!processing_ && facesNeeded);
                });

            auto *pathRow = new QHBoxLayout();
            pathRow->setSpacing(8);
            modelPathEdit_ = new QLineEdit(card);
            modelPathEdit_->setReadOnly(true);
            addRetranslation(
                [this]
                {
                    modelPathEdit_->setPlaceholderText(tr("Face model path"));
                });
            downloadButton_ = new QPushButton(card);
            downloadButton_->setObjectName("primaryButton");
            addRetranslation(
                [this]
                {
                    downloadButton_->setText(tr("Download"));
                });
            downloadButton_->setCursor(Qt::PointingHandCursor);
            downloadButton_->setVisible(false);
            modelBrowseButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    modelBrowseButton_->setText(tr("Browse…"));
                });
            modelBrowseButton_->setCursor(Qt::PointingHandCursor);
            pathRow->addWidget(modelPathEdit_, 1);
            pathRow->addWidget(downloadButton_);
            pathRow->addWidget(modelBrowseButton_);
            cardLayout->addLayout(pathRow);

            connect(
                downloadButton_, &QPushButton::clicked, this, &MainWindow::downloadSelectedModel);
            connect(modelBrowseButton_, &QPushButton::clicked, this, &MainWindow::chooseModel);
            connect(modelCombo_,
                &QComboBox::currentIndexChanged,
                this,
                &MainWindow::updateModelPathFromSelection);

            root->addWidget(card);
        }

        {
            auto *card = makeCard(central);
            auto *cardLayout = new QVBoxLayout(card);
            cardLayout->setContentsMargins(20, 18, 20, 18);
            cardLayout->setSpacing(12);

            auto *inputsTitle = makeSectionTitle(card);
            cardLayout->addWidget(inputsTitle);
            addRetranslation(
                [inputsTitle]
                {
                    inputsTitle->setText(tr("Inputs"));
                });

            auto *inputsHint = makeSectionHint(card);
            cardLayout->addWidget(inputsHint);
            addRetranslation(
                [inputsHint]
                {
                    inputsHint->setText(
                        tr("Drag images, videos, or folders here, or use the buttons below."));
                });

            auto *dropList = new DropListWidget(card);
            inputList_ = dropList;
            inputList_->setSelectionMode(QAbstractItemView::ExtendedSelection);
            inputList_->setMinimumHeight(140);
            inputList_->setAlternatingRowColors(false);
            inputList_->setIconSize(QSize(40, 40));
            addRetranslation(
                [this, dropList]
                {
                    dropList->setPlaceholderText(tr("Drop images, videos, or folders here"));
                    inputList_->setAccessibleName(tr("Input images and folders"));
                    inputList_->setToolTip(
                        tr("Right-click for options · Delete removes selected items"));
                });
            cardLayout->addWidget(inputList_);

            auto *buttonRow = new QHBoxLayout();
            buttonRow->setSpacing(8);
            addFilesButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    addFilesButton_->setText(tr("Add Files"));
                });
            addFolderButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    addFolderButton_->setText(tr("Add Folder"));
                });
            clearInputsButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    clearInputsButton_->setText(tr("Clear"));
                });
            addFilesButton_->setCursor(Qt::PointingHandCursor);
            addFolderButton_->setCursor(Qt::PointingHandCursor);
            clearInputsButton_->setCursor(Qt::PointingHandCursor);
            recursiveCheck_ = new QCheckBox(card);
            addRetranslation(
                [this]
                {
                    recursiveCheck_->setText(tr("Include subfolders"));
                });
            recursiveCheck_->setChecked(true);
            reviewCheck_ = new QCheckBox(card);
            addRetranslation(
                [this]
                {
                    reviewCheck_->setText(tr("Review before saving"));
                });
            reviewCheck_->setChecked(true);
            addRetranslation(
                [this]
                {
                    reviewCheck_->setToolTip(
                        tr("Review detections before output:\n"
                           "  • Images: exclude boxes or add missed regions\n"
                           "  • Videos: scrub the timeline, exclude false tracks, or add "
                           "missed tracks with keyframes"));
                });

            connect(addFilesButton_, &QPushButton::clicked, this, &MainWindow::chooseFiles);
            connect(addFolderButton_, &QPushButton::clicked, this, &MainWindow::chooseFolder);
            connect(clearInputsButton_, &QPushButton::clicked, inputList_, &QListWidget::clear);

            buttonRow->addWidget(addFilesButton_);
            buttonRow->addWidget(addFolderButton_);
            buttonRow->addWidget(clearInputsButton_);
            buttonRow->addStretch(1);
            buttonRow->addWidget(reviewCheck_);
            buttonRow->addWidget(recursiveCheck_);
            cardLayout->addLayout(buttonRow);

            root->addWidget(card);
        }

        {
            auto *card = makeCard(central);
            auto *cardLayout = new QVBoxLayout(card);
            cardLayout->setContentsMargins(20, 18, 20, 18);
            cardLayout->setSpacing(12);

            auto *outputTitle = makeSectionTitle(card);
            cardLayout->addWidget(outputTitle);
            addRetranslation(
                [outputTitle]
                {
                    outputTitle->setText(tr("Output"));
                });

            auto *outputHint = makeSectionHint(card);
            cardLayout->addWidget(outputHint);
            addRetranslation(
                [outputHint]
                {
                    outputHint->setText(
                        tr("Anonymized copies are written here, preserving folder structure."));
                });

            auto *outputRow = new QHBoxLayout();
            outputRow->setSpacing(8);
            outputDirEdit_ = new QLineEdit(defaultOutputDirectory(), card);
            outputBrowseButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    outputBrowseButton_->setText(tr("Choose…"));
                });
            outputBrowseButton_->setCursor(Qt::PointingHandCursor);
            outputRow->addWidget(outputDirEdit_, 1);
            outputRow->addWidget(outputBrowseButton_);
            cardLayout->addLayout(outputRow);
            connect(outputBrowseButton_,
                &QPushButton::clicked,
                this,
                &MainWindow::chooseOutputDirectory);

            preserveMetaCheck_ = new QCheckBox(card);
            addRetranslation(
                [this]
                {
                    preserveMetaCheck_->setText(tr("Preserve selected EXIF metadata"));
                });
            preserveMetaCheck_->setChecked(false);
            addRetranslation(
                [this]
                {
                    if (metadataSupportAvailable())
                    {
                        preserveMetaCheck_->setToolTip(
                            tr("Off (default): output carries no metadata — GPS, camera, and "
                               "timestamps are removed.\n"
                               "On: copies selected EXIF fields such as camera, timestamps, and "
                               "location. Embedded "
                               "previews, IPTC, XMP, comments, and color profiles are removed. "
                               "Format and bit depth "
                               "are preserved at maximum quality."));
                    }
                    else
                    {
                        preserveMetaCheck_->setToolTip(
                            tr("Metadata preservation is unavailable in this build. Output "
                               "metadata will be removed."));
                    }
                });
            preserveMetaCheck_->setEnabled(metadataSupportAvailable());
            cardLayout->addWidget(preserveMetaCheck_);

            root->addWidget(card);
        }

        {
            auto *card = makeCard(central);
            auto *cardLayout = new QVBoxLayout(card);
            cardLayout->setContentsMargins(20, 14, 20, 14);
            cardLayout->setSpacing(0);

            auto *headerRow = new QHBoxLayout();
            headerRow->setSpacing(8);
            headerRow->setContentsMargins(0, 0, 0, 0);

            advancedToggle_ = new QToolButton(card);
            advancedToggle_->setObjectName("advancedToggle");
            addRetranslation(
                [this]
                {
                    advancedToggle_->setText(tr("Advanced Options"));
                });
            advancedToggle_->setCheckable(true);
            advancedToggle_->setChecked(false);
            advancedToggle_->setArrowType(Qt::RightArrow);
            advancedToggle_->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
            advancedToggle_->setCursor(Qt::PointingHandCursor);

            resetAdvancedButton_ = new QPushButton(card);
            addRetranslation(
                [this]
                {
                    resetAdvancedButton_->setText(tr("Reset to defaults"));
                });
            resetAdvancedButton_->setCursor(Qt::PointingHandCursor);

            headerRow->addWidget(advancedToggle_);
            headerRow->addStretch(1);
            headerRow->addWidget(resetAdvancedButton_);
            cardLayout->addLayout(headerRow);

            advancedBody_ = new QWidget(card);
            auto *bodyLayout = new QVBoxLayout(advancedBody_);
            bodyLayout->setContentsMargins(0, 14, 0, 4);
            bodyLayout->setSpacing(12);

            auto *advancedHint = makeSectionHint(advancedBody_);
            bodyLayout->addWidget(advancedHint);
            addRetranslation(
                [advancedHint]
                {
                    advancedHint->setText(tr("Tweak detection and anonymization behavior. Defaults "
                                             "work for most photos."));
                });

            methodCombo_ = new QComboBox(advancedBody_);
            methodCombo_->addItem(QString(), static_cast<int>(AnonymizationMethod::Mosaic));
            methodCombo_->addItem(QString(), static_cast<int>(AnonymizationMethod::Blur));
            methodCombo_->addItem(QString(), static_cast<int>(AnonymizationMethod::Fill));
            methodCombo_->addItem(QString(), static_cast<int>(AnonymizationMethod::CustomImage));
            addRetranslation(
                [this]
                {
                    methodCombo_->setItemText(0, tr("Mosaic (pixelate)"));
                    methodCombo_->setItemText(1, tr("Gaussian blur"));
                    methodCombo_->setItemText(2, tr("Solid fill (blackout)"));
                    methodCombo_->setItemText(3, tr("Custom image"));
                });
            addRetranslation(
                [this]
                {
                    methodCombo_->setToolTip(
                        tr("How detected faces are obscured.\n"
                           "Mosaic = pixelation (block size below).\n"
                           "Gaussian blur = strong smoothing scaled to face size.\n"
                           "Solid fill = opaque black box, irreversible.\n"
                           "Custom image = place your selected image over every detected region. "
                           "Default: Mosaic"));
                });

            customImagePicker_ = new QWidget(advancedBody_);
            auto *customImageLayout = new QHBoxLayout(customImagePicker_);
            customImageLayout->setContentsMargins(0, 0, 0, 0);
            customImageLayout->setSpacing(8);
            customImagePathEdit_ = new QLineEdit(customImagePicker_);
            customImagePathEdit_->setReadOnly(true);
            customImageBrowseButton_ = new QPushButton(customImagePicker_);
            customImageBrowseButton_->setCursor(Qt::PointingHandCursor);
            customImageLayout->addWidget(customImagePathEdit_, 1);
            customImageLayout->addWidget(customImageBrowseButton_);
            addRetranslation(
                [this]
                {
                    customImagePathEdit_->setPlaceholderText(
                        tr("Choose an image to cover detected faces"));
                    customImagePathEdit_->setToolTip(
                        tr("The image keeps its aspect ratio and follows detected face tilt "
                           "when available. Transparent pixels reveal a safety mosaic instead "
                           "of the original image."));
                    customImageBrowseButton_->setText(tr("Browse…"));
                    customImageBrowseButton_->setAccessibleName(tr("Choose custom image"));
                });

            shapeCombo_ = new QComboBox(advancedBody_);
            shapeCombo_->addItem(QString(), static_cast<int>(MaskShape::Rectangle));
            shapeCombo_->addItem(QString(), static_cast<int>(MaskShape::Ellipse));
            addRetranslation(
                [this]
                {
                    shapeCombo_->setItemText(0, tr("Rectangle"));
                    shapeCombo_->setItemText(1, tr("Rounded (ellipse)"));
                });
            addRetranslation(
                [this]
                {
                    shapeCombo_->setToolTip(tr("Shape of the obscured region.\n"
                                               "Rectangle = full padded box.\n"
                                               "Rounded = elliptical mask that follows the face "
                                               "and leaves corners untouched. "
                                               "Default: Rectangle"));
                });

            softEdgeCheck_ = new QCheckBox(advancedBody_);
            addRetranslation(
                [this]
                {
                    softEdgeCheck_->setAccessibleName(tr("Soft edges"));
                    softEdgeCheck_->setToolTip(
                        tr("Fades the edge of the obscured region into the photo "
                           "instead of a hard cutoff.\n"
                           "The fade only extends outward, so the detected area "
                           "stays fully covered. Default: off"));
                });

            scoreThresholdSpin_ = new QDoubleSpinBox(advancedBody_);
            scoreThresholdSpin_->setRange(0.05, 0.99);
            scoreThresholdSpin_->setSingleStep(0.05);
            scoreThresholdSpin_->setDecimals(2);
            scoreThresholdSpin_->setValue(kDefaultScoreThreshold);
            addRetranslation(
                [this]
                {
                    scoreThresholdSpin_->setToolTip(tr(
                        "Minimum confidence to accept a face.\n"
                        "Higher = fewer false positives but may miss small or side-profile faces.\n"
                        "Lower = catches more faces but may blur non-face regions. Default: 0.50"));
                });

            nmsThresholdSpin_ = new QDoubleSpinBox(advancedBody_);
            nmsThresholdSpin_->setRange(0.05, 0.95);
            nmsThresholdSpin_->setSingleStep(0.05);
            nmsThresholdSpin_->setDecimals(2);
            nmsThresholdSpin_->setValue(kDefaultNmsThreshold);
            addRetranslation(
                [this]
                {
                    nmsThresholdSpin_->setToolTip(
                        tr("Non-Maximum Suppression overlap threshold for duplicate boxes.\n"
                           "Lower = more aggressively removes overlapping detections.\n"
                           "Higher = allows more overlap. Default: 0.40"));
                });

            blockSizeSpin_ = new QSpinBox(advancedBody_);
            blockSizeSpin_->setRange(2, 200);
            blockSizeSpin_->setValue(kDefaultBlockSize);
            addRetranslation(
                [this]
                {
                    blockSizeSpin_->setToolTip(
                        tr("Mosaic block size in pixels.\n"
                           "Larger = coarser blocks, harder to un-blur.\n"
                           "Smaller = finer mosaic, higher recovery risk. Default: 14"));
                });

            paddingSpin_ = new QDoubleSpinBox(advancedBody_);
            paddingSpin_->setRange(0.0, 1.0);
            paddingSpin_->setSingleStep(0.05);
            paddingSpin_->setDecimals(2);
            paddingSpin_->setValue(kDefaultPadding);
            addRetranslation(
                [this]
                {
                    paddingSpin_->setToolTip(
                        tr("Extra margin around each detected face, as a fraction of its size.\n"
                           "Covers ears, hairline, and chin that the detector may miss.\n"
                           "0.00 = exact box, 0.18 = ~18% larger. Default: 0.18"));
                });

            auto *grid = new QFormLayout();
            grid->setLabelAlignment(Qt::AlignLeft);
            grid->setFormAlignment(Qt::AlignLeft | Qt::AlignTop);
            grid->setHorizontalSpacing(18);
            grid->setVerticalSpacing(10);
            auto *methodLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [methodLabel]
                {
                    methodLabel->setText(tr("Anonymization"));
                });
            grid->addRow(methodLabel, methodCombo_);
            customImageLabel_ = makeFieldLabel(advancedBody_);
            addRetranslation(
                [this]
                {
                    customImageLabel_->setText(tr("Custom image"));
                });
            grid->addRow(customImageLabel_, customImagePicker_);
            auto *shapeLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [shapeLabel]
                {
                    shapeLabel->setText(tr("Shape"));
                });
            grid->addRow(shapeLabel, shapeCombo_);
            auto *softEdgeLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [softEdgeLabel]
                {
                    softEdgeLabel->setText(tr("Soft edges"));
                });
            grid->addRow(softEdgeLabel, softEdgeCheck_);
            auto *scoreLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [scoreLabel]
                {
                    scoreLabel->setText(tr("Score threshold"));
                });
            grid->addRow(scoreLabel, scoreThresholdSpin_);
            auto *nmsLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [nmsLabel]
                {
                    nmsLabel->setText(tr("NMS threshold"));
                });
            grid->addRow(nmsLabel, nmsThresholdSpin_);
            auto *blockLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [blockLabel]
                {
                    blockLabel->setText(tr("Mosaic block size"));
                });
            grid->addRow(blockLabel, blockSizeSpin_);
            auto *paddingLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [paddingLabel]
                {
                    paddingLabel->setText(tr("Face padding"));
                });
            grid->addRow(paddingLabel, paddingSpin_);

            samplePreview_ = new QLabel(advancedBody_);
            samplePreview_->setFixedSize(152, 96);
            addRetranslation(
                [this]
                {
                    samplePreview_->setAccessibleName(tr("Anonymization style preview"));
                    samplePreview_->setToolTip(
                        tr("Sample of the current anonymization style and block size."));
                });
            auto *sampleLabel = makeFieldLabel(advancedBody_);
            addRetranslation(
                [sampleLabel]
                {
                    sampleLabel->setText(tr("Preview"));
                });
            grid->addRow(sampleLabel, samplePreview_);
            bodyLayout->addLayout(grid);

            advancedBody_->setVisible(false);
            cardLayout->addWidget(advancedBody_);

            connect(advancedToggle_, &QToolButton::toggled, this, &MainWindow::toggleAdvanced);
            connect(resetAdvancedButton_,
                &QPushButton::clicked,
                this,
                &MainWindow::resetAdvancedDefaults);
            connect(methodCombo_,
                &QComboBox::currentIndexChanged,
                this,
                [this]
                {
                    updateAnonymizationControls();
                    updateAnonymizationSample();
                });
            connect(customImageBrowseButton_,
                &QPushButton::clicked,
                this,
                &MainWindow::chooseCustomImage);
            connect(shapeCombo_,
                &QComboBox::currentIndexChanged,
                this,
                [this]
                {
                    updateAnonymizationSample();
                });
            connect(softEdgeCheck_,
                &QCheckBox::toggled,
                this,
                [this]
                {
                    updateAnonymizationSample();
                });
            connect(blockSizeSpin_,
                &QSpinBox::valueChanged,
                this,
                [this]
                {
                    updateAnonymizationSample();
                });
            connect(paddingSpin_,
                &QDoubleSpinBox::valueChanged,
                this,
                [this]
                {
                    updateAnonymizationSample();
                });
            updateAnonymizationControls();
            updateAnonymizationSample();

            root->addWidget(card);
        }

        {
            auto *card = makeCard(central);
            auto *cardLayout = new QVBoxLayout(card);
            cardLayout->setContentsMargins(20, 18, 20, 18);
            cardLayout->setSpacing(10);

            auto *activityTitle = makeSectionTitle(card);
            cardLayout->addWidget(activityTitle);
            addRetranslation(
                [activityTitle]
                {
                    activityTitle->setText(tr("Activity"));
                });
            logEdit_ = new QPlainTextEdit(card);
            logEdit_->setReadOnly(true);
            logEdit_->setMinimumHeight(140);

            logEdit_->setMaximumBlockCount(5000);
            cardLayout->addWidget(logEdit_);
            root->addWidget(card);
        }

        scrollArea->setWidget(central);
        containerLayout->addWidget(scrollArea, 1);

        auto *bottomBar = new QWidget(container);
        bottomBar->setObjectName("bottomBar");
        bottomBar->setAutoFillBackground(true);
        auto *bottomLayout = new QHBoxLayout(bottomBar);
        bottomLayout->setContentsMargins(24, 14, 24, 14);
        bottomLayout->setSpacing(14);

        statusLabel_ = new QLabel(bottomBar);
        statusLabel_->setObjectName("statusLabel");
        addRetranslation(
            [this]
            {
                statusLabel_->setText(tr("Ready"));
            });

        progressBar_ = new QProgressBar(bottomBar);
        progressBar_->setRange(0, 100);
        progressBar_->setValue(0);
        progressBar_->setTextVisible(false);
        progressBar_->setFixedHeight(6);
        addRetranslation(
            [this]
            {
                progressBar_->setAccessibleName(tr("Processing progress"));
            });

        openOutputButton_ = new QPushButton(bottomBar);
        addRetranslation(
            [this]
            {
                openOutputButton_->setText(tr("Open Output Folder"));
            });
        openOutputButton_->setCursor(Qt::PointingHandCursor);
        openOutputButton_->setVisible(false);
        connect(openOutputButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                const QString dir = outputDirEdit_->text();
                if (!dir.isEmpty() && QFileInfo::exists(dir))
                {
                    QDesktopServices::openUrl(QUrl::fromLocalFile(dir));
                }
            });

        stopButton_ = new QPushButton(bottomBar);
        stopButton_->setObjectName("dangerButton");
        addRetranslation(
            [this]
            {
                stopButton_->setText(tr("Stop"));
            });
        stopButton_->setCursor(Qt::PointingHandCursor);
        stopButton_->setEnabled(false);

        startButton_ = new QPushButton(bottomBar);
        startButton_->setObjectName("primaryButton");
        addRetranslation(
            [this]
            {
                startButton_->setText(tr("Start"));
            });
        startButton_->setCursor(Qt::PointingHandCursor);
        startButton_->setDefault(true);

        connect(startButton_, &QPushButton::clicked, this, &MainWindow::startProcessing);
        connect(stopButton_, &QPushButton::clicked, this, &MainWindow::stopProcessing);

        bottomLayout->addWidget(statusLabel_);
        bottomLayout->addWidget(progressBar_, 1);
        bottomLayout->addWidget(openOutputButton_);
        bottomLayout->addWidget(stopButton_);
        bottomLayout->addWidget(startButton_);

        containerLayout->addWidget(bottomBar);

        setCentralWidget(container);
        statusBar()->hide();

        populateBundledModels();
        loadSettings();
        updateSettingsIcon();
        appendLog(tr("Ready. Drop images, videos, or folders to begin."));

#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
        connect(qApp->styleHints(),
            &QStyleHints::colorSchemeChanged,
            this,
            [this](Qt::ColorScheme)
            {
                if (themeMode_ == ThemeMode::System)
                {
                    applyTheme(*qApp, ThemeMode::System);
                    updateSettingsIcon();
                }
            });
#endif

        checkForUpdates();
    }

    MainWindow::~MainWindow()
    {
        shuttingDown_ = true;
        QPointer<QThread> thread(workerThread_);
        QPointer<ProcessorWorker> worker(worker_);
        workerThread_ = nullptr;
        worker_ = nullptr;
        activeRunState_.reset();

        if (worker)
        {
            QObject::disconnect(worker, nullptr, this, nullptr);
            worker->cancel();
        }
        if (thread)
        {
            thread->quit();
            while (thread && !thread->wait(50))
            {
                QCoreApplication::processEvents(QEventLoop::ExcludeUserInputEvents);
            }
        }
    }

    namespace
    {
        bool acceptableDropPath(const QFileInfo &info)
        {
            if (!info.exists())
            {
                return false;
            }
            if (info.isDir())
            {
                return true;
            }
            if (!info.isFile())
            {
                return false;
            }
            const auto path = pathFromQString(info.filePath());
            return isSupportedImage(path) || isSupportedVideo(path);
        }

        bool hasAcceptableLocalUrls(const QMimeData *mime)
        {
            if (mime == nullptr || !mime->hasUrls())
            {
                return false;
            }
            for (const auto &url : mime->urls())
            {
                if (!url.isLocalFile())
                {
                    continue;
                }
                if (acceptableDropPath(QFileInfo(url.toLocalFile())))
                {
                    return true;
                }
            }
            return false;
        }
    }

    void MainWindow::dragEnterEvent(QDragEnterEvent *event)
    {
        if (!processing_ && hasAcceptableLocalUrls(event->mimeData()))
        {
            setDropHighlight(true);
            event->acceptProposedAction();
        }
        else
        {
            event->ignore();
        }
    }

    void MainWindow::dragLeaveEvent(QDragLeaveEvent *event)
    {
        setDropHighlight(false);
        QMainWindow::dragLeaveEvent(event);
    }

    void MainWindow::dropEvent(QDropEvent *event)
    {
        setDropHighlight(false);
        if (processing_ || !hasAcceptableLocalUrls(event->mimeData()))
        {
            event->ignore();
            return;
        }
        int unsupportedCount = 0;
        for (const auto &url : event->mimeData()->urls())
        {
            if (!url.isLocalFile())
            {
                continue;
            }
            const QFileInfo info(url.toLocalFile());
            if (acceptableDropPath(info))
            {
                addInputPath(url.toLocalFile());
            }
            else if (info.exists() && info.isFile())
            {
                ++unsupportedCount;
            }
        }
        if (unsupportedCount > 0)
        {
            appendLog(tr("Ignored %n unsupported file(s).", nullptr, unsupportedCount));
        }
        event->acceptProposedAction();
    }

    void MainWindow::chooseModel()
    {
        if (processing_)
        {
            return;
        }
        const auto path = QFileDialog::getOpenFileName(
            this, tr("Select SCRFD ONNX Model"), QDir::currentPath(), tr("ONNX Models (*.onnx)"));
        if (!path.isEmpty())
        {
            if (!customModelFileIsAllowed(this, path) || !confirmTrustedCustomModel(this, path))
            {
                return;
            }
            const auto approval = approvalForCustomModel(path);
            if (!approval)
            {
                appendLog(tr("Could not read the custom model file."));
                return;
            }
            customModelApproval_ = *approval;
            const QFileInfo info(path);
            modelCombo_->addItem(tr("Custom — %1").arg(info.fileName()), info.absoluteFilePath());
            modelCombo_->setItemData(modelCombo_->count() - 1, -1, kModelCatalogIndexRole);
            modelCombo_->setCurrentIndex(modelCombo_->count() - 1);
            modelPathEdit_->setText(path);
        }
    }

    void MainWindow::chooseCustomImage()
    {
        if (processing_)
        {
            return;
        }
        const QString initialDirectory =
            customImagePathEdit_->text().isEmpty()
                ? QStandardPaths::writableLocation(QStandardPaths::PicturesLocation)
                : QFileInfo(customImagePathEdit_->text()).absolutePath();
        const QString path = QFileDialog::getOpenFileName(this,
            tr("Select Custom Image"),
            initialDirectory,
            tr("Images (*.png *.jpg *.jpeg *.bmp *.tif *.tiff *.webp)"));
        if (path.isEmpty())
        {
            return;
        }

        auto loaded = loadCustomImage(path);
        if (loaded.image.empty())
        {
            QMessageBox::warning(this, tr("Invalid Custom Image"), loaded.error);
            return;
        }
        customImage_ = std::move(loaded.image);
        customImagePathEdit_->setText(loaded.canonicalPath);
        updateAnonymizationSample();
    }

    void MainWindow::chooseFiles()
    {
        if (processing_)
        {
            return;
        }
        const auto files = QFileDialog::getOpenFileNames(this,
            tr("Select Images or Videos"),
            QStandardPaths::writableLocation(QStandardPaths::PicturesLocation),
            tr("Images & Videos (*.jpg *.jpeg *.png *.bmp *.tif *.tiff *.webp *.mp4 *.mov *.m4v "
               "*.webm)"));
        for (const auto &file : files)
        {
            addInputPath(file);
        }
    }

    void MainWindow::chooseFolder()
    {
        if (processing_)
        {
            return;
        }
        const auto folder = QFileDialog::getExistingDirectory(this,
            tr("Select Folder"),
            QStandardPaths::writableLocation(QStandardPaths::PicturesLocation));
        if (!folder.isEmpty())
        {
            addInputPath(folder);
        }
    }

    void MainWindow::chooseOutputDirectory()
    {
        if (processing_)
        {
            return;
        }
        const auto folder = QFileDialog::getExistingDirectory(
            this, tr("Select Output Folder"), outputDirEdit_->text());
        if (!folder.isEmpty())
        {
            outputDirEdit_->setText(folder);
        }
    }

    MainWindow::DetectorCacheKey MainWindow::makeDetectorCacheKey(
        const QString &modelPath, bool gpuAcceleration, const FaceModelKind faceModelKind)
    {
        if (modelPath.isEmpty())
        {
            return {};
        }

        const QFileInfo info(modelPath);
        QString canonicalPath = info.canonicalFilePath();
        if (canonicalPath.isEmpty())
        {
            canonicalPath = QDir::cleanPath(info.absoluteFilePath());
        }

        DetectorCacheKey key;
        key.canonicalModelPath = canonicalPath;
        key.gpuAcceleration = gpuAcceleration;
        key.faceModelKind = faceModelKind;
        if (info.exists() && info.isFile() && info.size() > 0
            && info.size() <= kMaxCustomModelBytes)
        {
            key.modelSize = info.size();
            key.modelLastModifiedMs = info.lastModified().toMSecsSinceEpoch();
            QFile file(canonicalPath);
            if (file.open(QIODevice::ReadOnly))
            {
                QCryptographicHash hash(QCryptographicHash::Sha256);
                qint64 hashedBytes = 0;
                while (!file.atEnd())
                {
                    const QByteArray chunk = file.read(qint64{1024} * 1024);
                    if (chunk.isEmpty())
                    {
                        break;
                    }
                    if (hashedBytes > kMaxCustomModelBytes - chunk.size())
                    {
                        return key;
                    }
                    hashedBytes += chunk.size();
                    hash.addData(chunk);
                }
                if (file.error() == QFileDevice::NoError && hashedBytes == key.modelSize)
                {
                    key.modelSha256 = hash.result();
                }
            }
        }
        return key;
    }

    void MainWindow::startProcessing()
    {
        if (processing_ || worker_ != nullptr || workerThread_ != nullptr)
        {
            return;
        }
        const QString detectTarget =
            detectCombo_ ? detectCombo_->currentData().toString() : QStringLiteral("faces");
        const bool detectFaces = detectTarget != "plates";
        const bool detectPlates = detectTarget != "faces";
        const auto selectedMethod =
            static_cast<AnonymizationMethod>(methodCombo_->currentData().toInt());
        cv::Mat runCustomImage;
        if (selectedMethod == AnonymizationMethod::CustomImage)
        {
            auto loaded = loadCustomImage(customImagePathEdit_->text());
            if (loaded.image.empty())
            {
                reportValidationIssue(loaded.error, customImagePathEdit_);
                return;
            }
            customImage_ = std::move(loaded.image);
            customImagePathEdit_->setText(loaded.canonicalPath);
            runCustomImage = customImage_;
            updateAnonymizationSample();
        }

        auto modelPath = selectedModelPath();
        const BuiltinModel *selectedBuiltin = selectedBuiltinModel();
        const bool isCustom = !modelPath.isEmpty() && selectedBuiltin == nullptr;

        if (detectFaces)
        {
            if (modelPath.isEmpty())
            {
                reportValidationIssue(tr("Choose a face ONNX model first."), modelCombo_);
                return;
            }

            if (!QFileInfo::exists(modelPath))
            {
                const BuiltinModel *builtin = isCustom ? nullptr : selectedBuiltin;
                if (builtin == nullptr)
                {
                    appendLog(tr("Choose a valid face ONNX model first."));
                    return;
                }
                appendLog(tr("Downloading %1…").arg(builtin->fileName));
                if (!ensureBuiltinModelAvailable(this, *builtin, modelPath))
                {
                    appendLog(tr("Model download was cancelled or failed."));
                    return;
                }
                updateModelPathFromSelection();
                appendLog(tr("Model ready: %1").arg(builtin->fileName));
            }

            if (isCustom
                && (!customModelFileIsAllowed(this, modelPath)
                    || !ensureCustomModelStillApproved(modelPath)))
            {
                return;
            }
        }

        QString plateModelPath;
        if (detectPlates)
        {
            plateModelPath = firstExistingModelPath(plateModel().fileName);
            if (plateModelPath.isEmpty())
            {
                plateModelPath = modelCacheDir() + "/" + plateModel().fileName;
                appendLog(tr("Downloading %1…").arg(plateModel().fileName));
                if (!ensurePlateModelAvailable(this, plateModelPath))
                {
                    appendLog(tr("Model download was cancelled or failed."));
                    return;
                }
                appendLog(tr("Model ready: %1").arg(plateModel().fileName));
            }
        }

        if (inputList_->count() == 0)
        {
            reportValidationIssue(tr("Add at least one image or folder."), inputList_);
            return;
        }

        if (outputDirEdit_->text().isEmpty())
        {
            reportValidationIssue(tr("Choose an output folder."), outputDirEdit_);
            return;
        }

        const QString rawOutput = outputDirEdit_->text();
        const QFileInfo outputInfo(rawOutput);
        const QString canonicalOutput =
            outputInfo.exists() ? outputInfo.canonicalFilePath() : QDir::cleanPath(rawOutput);
        for (const auto &input : inputPaths())
        {
            const QFileInfo inputInfo(input);
            const QString canonicalInput =
                inputInfo.exists() ? inputInfo.canonicalFilePath() : QDir::cleanPath(input);
            if (canonicalInput.isEmpty() || canonicalOutput.isEmpty())
            {
                continue;
            }

            const bool isSame = canonicalInput.compare(canonicalOutput, Qt::CaseInsensitive) == 0;
            const QString withSep =
                canonicalInput.endsWith('/') ? canonicalInput : canonicalInput + '/';
            const bool isUnder = canonicalOutput.startsWith(withSep, Qt::CaseInsensitive);
            if (isSame || isUnder)
            {
                reportValidationIssue(
                    tr("Refusing to run: output folder is inside input '%1'. "
                       "Pick a different output folder so originals aren't overwritten.")
                        .arg(input),
                    outputDirEdit_);
                return;
            }
        }

        ProcessingRequest request;
        request.modelPath = modelPath;
        request.faceModelKind =
            selectedBuiltin != nullptr ? selectedBuiltin->faceKind : FaceModelKind::Scrfd;
        request.inputs = inputPaths();
        request.outputDirectory = outputDirEdit_->text();
        request.plateModelPath = plateModelPath;
        request.reviewReceiver = this;
        request.recursive = recursiveCheck_->isChecked();
        request.scoreThreshold = static_cast<float>(scoreThresholdSpin_->value());
        request.nmsThreshold = static_cast<float>(nmsThresholdSpin_->value());
        request.mosaicBlockSize = blockSizeSpin_->value();
        request.paddingRatio = static_cast<float>(paddingSpin_->value());
        request.method = selectedMethod;
        request.customImage = runCustomImage;
        request.shape = static_cast<MaskShape>(shapeCombo_->currentData().toInt());
        request.softEdges = softEdgeCheck_->isChecked();
        request.preserveMetadata = metadataSupportAvailable() && preserveMetaCheck_->isChecked();
        request.reviewEnabled = reviewCheck_->isChecked();
        request.detectFaces = detectFaces;
        request.detectPlates = detectPlates;
        request.gpuAcceleration = gpuAcceleration_;
        request.videoCrf = crfForQuality(static_cast<VideoQuality>(videoQuality_));
        request.videoCodec = static_cast<VideoCodec>(videoCodec_);

        ActiveRunState runState;
        runState.faceKey =
            detectFaces
                ? makeDetectorCacheKey(modelPath, request.gpuAcceleration, request.faceModelKind)
                : DetectorCacheKey{};
        runState.plateKey = detectPlates
                                ? makeDetectorCacheKey(plateModelPath, request.gpuAcceleration)
                                : DetectorCacheKey{};
        const auto verifyOrRecoverBuiltin =
            [&](const BuiltinModel &model, QString &path, DetectorCacheKey &key, const bool plate)
        {
            if (key.isValid() && modelDigestMatches(model, key.modelSha256))
            {
                return true;
            }

            appendLog(tr("Built-in model integrity check failed: %1").arg(model.fileName));
            const QString recoveryPath = modelCacheDir() + "/" + model.fileName;
            appendLog(tr("Downloading %1…").arg(model.fileName));
            const bool recovered = plate ? ensurePlateModelAvailable(this, recoveryPath)
                                         : ensureBuiltinModelAvailable(this, model, recoveryPath);
            if (!recovered)
            {
                appendLog(tr("Model download was cancelled or failed."));
                return false;
            }

            path = recoveryPath;
            key = makeDetectorCacheKey(path,
                request.gpuAcceleration,
                plate ? FaceModelKind::Scrfd : request.faceModelKind);
            if (!key.isValid() || !modelDigestMatches(model, key.modelSha256))
            {
                appendLog(tr("Built-in model integrity check failed: %1").arg(model.fileName));
                return false;
            }
            appendLog(tr("Model ready: %1").arg(model.fileName));
            return true;
        };

        if (detectFaces && selectedBuiltin != nullptr
            && !verifyOrRecoverBuiltin(*selectedBuiltin, modelPath, runState.faceKey, false))
        {
            return;
        }
        if (detectPlates
            && !verifyOrRecoverBuiltin(plateModel(), plateModelPath, runState.plateKey, true))
        {
            return;
        }
        if ((detectFaces && !runState.faceKey.isValid())
            || (detectPlates && !runState.plateKey.isValid()))
        {
            reportValidationIssue(tr("Choose a valid face ONNX model first."), modelCombo_);
            return;
        }
        if (detectFaces)
        {
            request.modelPath = runState.faceKey.canonicalModelPath;
            request.modelSha256 = runState.faceKey.modelSha256;
            if (selectedBuiltin != nullptr)
            {
                modelCombo_->setItemData(modelCombo_->currentIndex(), request.modelPath);
                updateModelPathFromSelection();
            }
        }
        if (detectPlates)
        {
            request.plateModelPath = runState.plateKey.canonicalModelPath;
            request.plateModelSha256 = runState.plateKey.modelSha256;
        }
        runState.method = request.method;
        runState.customImage = request.customImage;
        runState.shape = request.shape;
        runState.blockSize = request.mosaicBlockSize;
        runState.padding = request.paddingRatio;
        runState.softEdges = request.softEdges;
        runState.preserveMetadata = request.preserveMetadata;
        runState.detectFaces = request.detectFaces;
        runState.detectPlates = request.detectPlates;

        auto detectorForRun =
            detectFaces && cachedDetectorKey_ == runState.faceKey ? cachedDetector_ : nullptr;
        auto videoDetectorForRun = detectFaces && cachedVideoDetectorKey_ == runState.faceKey
                                       ? cachedVideoDetector_
                                       : nullptr;
        auto plateForRun = detectPlates && cachedPlateDetectorKey_ == runState.plateKey
                               ? cachedPlateDetector_
                               : nullptr;

        activeRunState_ = runState;
        setProcessing(true);
        openOutputButton_->setVisible(false);
        runTimer_.start();
        progressBar_->setValue(0);
        lastRunSummary_ = {};
        statusLabel_->setText(tr("Starting…"));

        workerThread_ = new QThread(this);

        DetectorCache cache;
        cache.face = std::move(detectorForRun);
        cache.plate = std::move(plateForRun);
        cache.videoFace = std::move(videoDetectorForRun);
        worker_ = new ProcessorWorker(std::move(request), std::move(cache));

        worker_->moveToThread(workerThread_);
        connect(workerThread_, &QThread::started, worker_, &ProcessorWorker::process);
        connect(worker_, &ProcessorWorker::logMessage, this, &MainWindow::appendLog);
        connect(worker_,
            &ProcessorWorker::summaryAvailable,
            this,
            [this](const RunSummary summary)
            {
                lastRunSummary_ = summary;
            });
        connect(worker_,
            &ProcessorWorker::progressChanged,
            this,
            [this](int completed, int total)
            {
                progressBar_->setRange(0, std::max(total, 1));
                progressBar_->setValue(completed);
            });
        connect(worker_,
            &ProcessorWorker::stageChanged,
            this,
            [this](int index, int total, const QString &stage, const QString &fileName)
            {
                const QString elided =
                    statusLabel_->fontMetrics().elidedText(fileName, Qt::ElideMiddle, 320);
                statusLabel_->setText(
                    QString("%1/%2  ·  %3  ·  %4").arg(index).arg(total).arg(stage, elided));
            });

        connect(worker_, &ProcessorWorker::finished, this, &MainWindow::onWorkerFinished);
        connect(workerThread_, &QThread::finished, worker_, &QObject::deleteLater);
        connect(workerThread_, &QThread::finished, workerThread_, &QObject::deleteLater);

        appendLog(tr("Starting…"));
        workerThread_->start();
    }

    void MainWindow::stopProcessing() const
    {
        if (worker_ != nullptr)
        {
            appendLog(tr("Stopping after the current processing step…"));
            statusLabel_->setText(tr("Stopping…"));
            worker_->cancel();
        }
    }

    void MainWindow::onWorkerFinished(RunOutcome outcome)
    {
        if (shuttingDown_)
        {
            return;
        }

        const auto completedRun = activeRunState_;
        const bool cacheRunResult =
            outcome == RunOutcome::Completed
            || (outcome == RunOutcome::CompletedWithWarnings && lastRunSummary_.failed == 0);

        if (worker_ != nullptr)
        {
            auto detector = worker_->takeDetector();
            if (detector && completedRun.has_value() && completedRun->detectFaces
                && completedRun->faceKey.isValid() && cacheRunResult)
            {
                cachedDetector_ = std::move(detector);
                cachedDetectorKey_ = completedRun->faceKey;
            }

            auto videoDetector = worker_->takeVideoDetector();
            if (videoDetector && completedRun.has_value() && completedRun->detectFaces
                && completedRun->faceKey.isValid() && cacheRunResult)
            {
                cachedVideoDetector_ = std::move(videoDetector);
                cachedVideoDetectorKey_ = completedRun->faceKey;
            }

            auto plate = worker_->takePlateDetector();
            if (plate && completedRun.has_value() && completedRun->detectPlates
                && completedRun->plateKey.isValid() && cacheRunResult)
            {
                cachedPlateDetector_ = std::move(plate);
                cachedPlateDetectorKey_ = completedRun->plateKey;
            }
        }

        QThread *completedThread = workerThread_;
        if (completedThread != nullptr)
        {
            completedThread->quit();
            completedThread->wait();
        }

        worker_ = nullptr;
        workerThread_ = nullptr;
        activeRunState_.reset();
        setProcessing(false);
        const qint64 seconds = runTimer_.isValid() ? runTimer_.elapsed() / 1000 : 0;
        const QString elapsed = seconds >= 60
                                    ? QStringLiteral("%1m %2s").arg(seconds / 60).arg(seconds % 60)
                                    : QStringLiteral("%1s").arg(seconds);
        switch (outcome)
        {
        case RunOutcome::Completed:
            appendLog(tr("Finished."));
            statusLabel_->setText(tr("Done") + QStringLiteral("  ·  ") + elapsed);
            openOutputButton_->setVisible(true);
            break;
        case RunOutcome::CompletedWithWarnings:
            appendLog(tr("Completed with warnings — review the results before sharing."));
            statusLabel_->setProperty("state", "warning");
            statusLabel_->style()->unpolish(statusLabel_);
            statusLabel_->style()->polish(statusLabel_);
            statusLabel_->setText(
                QStringLiteral("⚠  ") + tr("Review required") + QStringLiteral("  ·  ") + elapsed);
            openOutputButton_->setVisible(true);
            QMessageBox::warning(this,
                tr("Review Required"),
                tr("Processing finished, but some results need attention.\n\n"
                   "Total: %1\nRedacted: %2\nSaved without redaction: %3\nCopied: %4\n"
                   "Skipped: %5\nFailed: %6\n\n"
                   "Files with detection or tracking warnings: %7\n"
                   "Omitted detection regions: %8\nTracking gap frames (before review): %9\n"
                   "Dropped tracks: %10\nFiles with metadata warnings: %11\n"
                   "Unreadable input paths: %12\n\n"
                   "Check these results before sharing them.")
                    .arg(lastRunSummary_.total)
                    .arg(lastRunSummary_.redacted)
                    .arg(lastRunSummary_.unredacted)
                    .arg(lastRunSummary_.copied)
                    .arg(lastRunSummary_.skipped)
                    .arg(lastRunSummary_.failed)
                    .arg(lastRunSummary_.coverageWarningFiles)
                    .arg(lastRunSummary_.omittedRegions)
                    .arg(lastRunSummary_.trackingGapFrames)
                    .arg(lastRunSummary_.droppedTracks)
                    .arg(lastRunSummary_.warningFiles)
                    .arg(lastRunSummary_.unreadableInputs));
            break;
        case RunOutcome::Cancelled:
            appendLog(tr("Cancelled."));
            statusLabel_->setText(tr("Cancelled") + QStringLiteral("  ·  ") + elapsed);
            openOutputButton_->setVisible(true);
            break;
        case RunOutcome::Failed:
            appendLog(tr("Failed — check the log for details."));
            statusLabel_->setProperty("state", "warning");
            statusLabel_->style()->unpolish(statusLabel_);
            statusLabel_->style()->polish(statusLabel_);
            statusLabel_->setText(QStringLiteral("⚠  ") + tr("Failed — check the log"));
            break;
        }
    }

    void MainWindow::toggleAdvanced(bool expanded) const
    {
        if (advancedBody_ != nullptr)
        {
            advancedBody_->setVisible(expanded);
        }
        if (advancedToggle_ != nullptr)
        {
            advancedToggle_->setArrowType(expanded ? Qt::DownArrow : Qt::RightArrow);
        }
    }

    void MainWindow::resetAdvancedDefaults() const
    {
        if (processing_)
        {
            return;
        }
        scoreThresholdSpin_->setValue(kDefaultScoreThreshold);
        nmsThresholdSpin_->setValue(kDefaultNmsThreshold);
        blockSizeSpin_->setValue(kDefaultBlockSize);
        paddingSpin_->setValue(kDefaultPadding);
        methodCombo_->setCurrentIndex(0);
        shapeCombo_->setCurrentIndex(0);
        softEdgeCheck_->setChecked(false);
    }

    void MainWindow::closeEvent(QCloseEvent *event)
    {
        saveSettings();
        QMainWindow::closeEvent(event);
    }

    void MainWindow::loadSettings()
    {
        QSettings settings;

        settings.beginGroup("window");
        const auto geometry = settings.value("geometry").toByteArray();
        if (!geometry.isEmpty())
        {
            restoreGeometry(geometry);
        }
        settings.endGroup();

        settings.beginGroup("processing");
        int savedModelIndex = 0;
        const QString savedModelFileName = settings.value("faceModelFileName").toString();
        if (!savedModelFileName.isEmpty())
        {
            for (std::size_t index = 0; index < builtinModels().size(); ++index)
            {
                if (builtinModels()[index].fileName == savedModelFileName)
                {
                    savedModelIndex = static_cast<int>(index);
                    break;
                }
            }
        }
        else if (settings.contains("modelIndex"))
        {
            savedModelIndex = settings.value("modelIndex").toInt() == 0 ? 1 : 0;
        }
        if (modelCombo_ != nullptr && savedModelIndex >= 0
            && savedModelIndex < modelCombo_->count())
        {
            modelCombo_->setCurrentIndex(savedModelIndex);
        }
        const auto savedCustomModel = settings.value("customModelPath").toString();
        // The approval is restored as it was recorded, not re-derived from the file: re-reading
        // the bytes here would approve whatever happens to be at the path now, which is the
        // thing this record exists to catch. A settings file written before approvals were
        // content-bound leaves this empty, and the first run asks again.
        customModelApproval_.digest = settings.value("customModelDigest").toString();
        customModelApproval_.size = settings.value("customModelSize").toLongLong();
        if (!savedCustomModel.isEmpty() && QFileInfo::exists(savedCustomModel))
        {
            const QFileInfo info(savedCustomModel);
            modelCombo_->addItem(tr("Custom — %1").arg(info.fileName()), info.absoluteFilePath());
            modelCombo_->setItemData(modelCombo_->count() - 1, -1, kModelCatalogIndexRole);
            modelCombo_->setCurrentIndex(modelCombo_->count() - 1);
        }

        const auto outputDir = settings.value("outputDir").toString();
        if (!outputDir.isEmpty())
        {
            outputDirEdit_->setText(outputDir);
        }

        recursiveCheck_->setChecked(settings.value("recursive", true).toBool());
        reviewCheck_->setChecked(settings.value("review", true).toBool());
        preserveMetaCheck_->setChecked(
            metadataSupportAvailable() && settings.value("preserveMetadata", false).toBool());

        scoreThresholdSpin_->setValue(
            settings.value("scoreThreshold", kDefaultScoreThreshold).toDouble());
        nmsThresholdSpin_->setValue(
            settings.value("nmsThreshold", kDefaultNmsThreshold).toDouble());
        blockSizeSpin_->setValue(settings.value("blockSize", kDefaultBlockSize).toInt());
        paddingSpin_->setValue(settings.value("padding", kDefaultPadding).toDouble());

        const auto savedMethod = settings.value("method", 0).toInt();
        if (methodCombo_ != nullptr && savedMethod >= 0 && savedMethod < methodCombo_->count())
        {
            methodCombo_->setCurrentIndex(savedMethod);
        }

        const QString savedCustomImage = settings.value("customImagePath").toString();
        if (!savedCustomImage.isEmpty())
        {
            customImagePathEdit_->setText(savedCustomImage);
            auto loaded = loadCustomImage(savedCustomImage);
            if (!loaded.image.empty())
            {
                customImage_ = std::move(loaded.image);
                customImagePathEdit_->setText(loaded.canonicalPath);
            }
        }

        const auto savedShape = settings.value("shape", 0).toInt();
        if (shapeCombo_ != nullptr && savedShape >= 0 && savedShape < shapeCombo_->count())
        {
            shapeCombo_->setCurrentIndex(savedShape);
        }

        if (softEdgeCheck_ != nullptr)
        {
            softEdgeCheck_->setChecked(settings.value("softEdges", false).toBool());
        }

        const auto savedDetect = settings.value("detectTarget", 0).toInt();
        if (detectCombo_ != nullptr && savedDetect >= 0 && savedDetect < detectCombo_->count())
        {
            detectCombo_->setCurrentIndex(savedDetect);
        }

        if (settings.value("advancedExpanded", false).toBool())
        {
            advancedToggle_->setChecked(true);
        }
        settings.endGroup();

        themeMode_ = themeModeFromString(settings.value("theme", "system").toString());
        checkForUpdatesOnStartup_ = settings.value("checkForUpdates", true).toBool();
        fileLogging_ = settings.value("fileLogging", true).toBool();
        gpuAcceleration_ = settings.value("gpuAcceleration", true).toBool();
        videoQuality_ = std::clamp(settings.value("videoQuality", 0).toInt(), 0, 2);
        videoCodec_ = std::clamp(settings.value("videoCodec", 0).toInt(), 0, 1);

        const auto savedLanguage = settings.value("language").toString();
        QString language = savedLanguage;
        if (language.isEmpty())
        {
            switch (QLocale::system().language())
            {
            case QLocale::Korean:
                language = QStringLiteral("ko");
                break;
            case QLocale::Japanese:
                language = QStringLiteral("ja");
                break;
            case QLocale::Chinese:
            {
                const QLocale locale = QLocale::system();
                if (locale.script() == QLocale::SimplifiedHanScript
                    || locale.territory() == QLocale::China
                    || locale.territory() == QLocale::Singapore)
                {
                    language = QStringLiteral("zh_CN");
                }
                else
                {
                    language = QStringLiteral("en");
                }
                break;
            }
            default:
                language = QStringLiteral("en");
                break;
            }
        }
        applyLanguage(language);

        updateModelPathFromSelection();
        updateAnonymizationControls();
        updateAnonymizationSample();
    }

    void MainWindow::saveSettings() const
    {
        QSettings settings;

        settings.beginGroup("window");
        settings.setValue("geometry", saveGeometry());
        settings.endGroup();

        settings.beginGroup("processing");
        settings.setValue("modelIndex", modelCombo_ ? modelCombo_->currentIndex() : 0);

        const auto currentModel = selectedModelPath();
        const BuiltinModel *builtin = selectedBuiltinModel();
        if (builtin != nullptr)
        {
            settings.setValue("faceModelFileName", builtin->fileName);
        }
        else
        {
            settings.remove("faceModelFileName");
        }
        if (!currentModel.isEmpty() && builtin == nullptr)
        {
            settings.setValue("customModelPath", currentModel);
            settings.setValue("customModelDigest", customModelApproval_.digest);
            settings.setValue("customModelSize", customModelApproval_.size);
        }
        else
        {
            settings.remove("customModelPath");
            settings.remove("customModelDigest");
            settings.remove("customModelSize");
        }

        settings.setValue("outputDir", outputDirEdit_->text());
        settings.setValue("recursive", recursiveCheck_->isChecked());
        settings.setValue("review", reviewCheck_->isChecked());
        settings.setValue("preserveMetadata", preserveMetaCheck_->isChecked());
        settings.setValue("scoreThreshold", scoreThresholdSpin_->value());
        settings.setValue("nmsThreshold", nmsThresholdSpin_->value());
        settings.setValue("blockSize", blockSizeSpin_->value());
        settings.setValue("padding", paddingSpin_->value());
        settings.setValue("method", methodCombo_ ? methodCombo_->currentIndex() : 0);
        settings.setValue(
            "customImagePath", customImagePathEdit_ ? customImagePathEdit_->text() : QString());
        settings.setValue("shape", shapeCombo_ ? shapeCombo_->currentIndex() : 0);
        settings.setValue("softEdges", softEdgeCheck_ != nullptr && softEdgeCheck_->isChecked());
        settings.setValue("detectTarget", detectCombo_ ? detectCombo_->currentIndex() : 0);
        settings.setValue(
            "advancedExpanded", advancedToggle_ ? advancedToggle_->isChecked() : false);
        settings.endGroup();

        settings.setValue("theme", themeModeToString(themeMode_));
        settings.setValue("checkForUpdates", checkForUpdatesOnStartup_);
        settings.setValue("fileLogging", fileLogging_);
        settings.setValue("gpuAcceleration", gpuAcceleration_);
        settings.setValue("videoQuality", videoQuality_);
        settings.setValue("videoCodec", videoCodec_);

        settings.setValue("language", language_);
    }

    void MainWindow::checkForUpdates()
    {
        if (!checkForUpdatesOnStartup_)
        {
            return;
        }

        auto *updater = SelfUpdater::create(this);
        if (updater == nullptr)
        {
            startLegacyUpdateCheck();
            return;
        }
        if (updater->mode() == SelfUpdater::Mode::OwnUi)
        {
            updater->checkForUpdates();
            return;
        }
        connect(updater,
            &SelfUpdater::checkFailed,
            this,
            [this, updater](const QString &error)
            {
                spdlog::info("self-update unavailable, falling back to update notification: {}",
                    error.toStdString());
                updater->deleteLater();
                startLegacyUpdateCheck();
            });
        connect(updater,
            &SelfUpdater::updateAvailable,
            this,
            [this, updater](const QString &version, const QString &releaseNotes)
            {
                showUpdateBanner(version, UpdateChecker::releasesPageUrl());
                if (askToUpdate(version, releaseNotes))
                {
                    downloadUpdateWithProgress(updater, version);
                }
            });
        updater->checkForUpdates();
    }

    void MainWindow::startLegacyUpdateCheck()
    {
        auto *checker = new UpdateChecker(QCoreApplication::applicationVersion(), this);
        connect(checker,
            &UpdateChecker::updateAvailable,
            this,
            [this](const QString &latestVersion,
                const QString &releaseUrl,
                const QString &releaseNotes)
            {
                showUpdateBanner(latestVersion, releaseUrl);
                if (askToUpdate(latestVersion, releaseNotes))
                {
                    QDesktopServices::openUrl(QUrl(releaseUrl));
                }
            });
        checker->check();
    }

    void MainWindow::showUpdateBanner(const QString &latestVersion, const QString &releaseUrl)
    {
        if (updateLabel_ == nullptr)
        {
            return;
        }
        const QString text = tr("Update available: %1").arg(latestVersion);
        updateLabel_->setText(QStringLiteral("<a href=\"%1\">%2</a>")
                .arg(releaseUrl.toHtmlEscaped(), text.toHtmlEscaped()));
        updateLabel_->setVisible(true);
    }

    bool MainWindow::askToUpdate(const QString &latestVersion, const QString &releaseNotes)
    {
        QMessageBox message(this);
        message.setWindowTitle(tr("Update Available"));
        message.setIcon(QMessageBox::Information);
        message.setTextFormat(Qt::PlainText);
        message.setText(tr("CloakFrame %1 is available. What's new:").arg(latestVersion));
        const QString localizedNotes = releaseNotesSection(releaseNotes);
        message.setInformativeText(localizedNotes.isEmpty()
                                       ? tr("No release notes were provided for this update.")
                                       : localizedNotes);

        auto *updateButton = message.addButton(tr("Update"), QMessageBox::AcceptRole);
        auto *laterButton = message.addButton(tr("Later"), QMessageBox::RejectRole);
        message.setDefaultButton(laterButton);
        message.setEscapeButton(laterButton);
        message.exec();

        return message.clickedButton() == updateButton;
    }

    void MainWindow::downloadUpdateWithProgress(SelfUpdater *updater, const QString &version)
    {
        QPointer<QProgressDialog> progress = new QProgressDialog(
            tr("Downloading CloakFrame %1…").arg(version), QString(), 0, 100, this);
        progress->setWindowModality(Qt::WindowModal);
        progress->setCancelButton(nullptr);
        progress->setMinimumDuration(0);
        progress->setAutoClose(false);
        progress->setAutoReset(false);
        connect(updater, &SelfUpdater::downloadProgress, progress, &QProgressDialog::setValue);
        connect(updater,
            &SelfUpdater::downloadFailed,
            this,
            [this, updater, progress](const QString &error)
            {
                if (progress)
                {
                    progress->close();
                    progress->deleteLater();
                }

                QMessageBox message(this);
                message.setWindowTitle(tr("Update Failed"));
                message.setIcon(QMessageBox::Warning);
                message.setTextFormat(Qt::PlainText);
                message.setText(tr("The update could not be installed: %1").arg(error));
                auto *openButton =
                    message.addButton(tr("Open Download Page"), QMessageBox::AcceptRole);
                message.addButton(QMessageBox::Close);
                message.exec();
                if (message.clickedButton() == openButton)
                {
                    QDesktopServices::openUrl(QUrl(UpdateChecker::releasesPageUrl()));
                }
                updater->deleteLater();
            });
        connect(updater,
            &SelfUpdater::downloadFinished,
            this,
            [this, updater, progress, version]
            {
                if (progress)
                {
                    progress->close();
                    progress->deleteLater();
                }

                if (processing_)
                {
                    QMessageBox::information(this,
                        tr("Update Ready"),
                        tr("CloakFrame %1 will finish installing the next time the app starts.")
                            .arg(version));
                    return;
                }

                QMessageBox message(this);
                message.setWindowTitle(tr("Update Ready"));
                message.setIcon(QMessageBox::Information);
                message.setTextFormat(Qt::PlainText);
                message.setText(
                    tr("CloakFrame %1 has been downloaded. Restart now to finish installing?")
                        .arg(version));
                auto *restartButton = message.addButton(tr("Restart Now"), QMessageBox::AcceptRole);
                auto *laterButton = message.addButton(tr("Later"), QMessageBox::RejectRole);
                message.setDefaultButton(restartButton);
                message.setEscapeButton(laterButton);
                message.exec();

                if (message.clickedButton() == restartButton)
                {
                    connect(updater,
                        &SelfUpdater::readyToQuit,
                        qApp,
                        &QCoreApplication::quit,
                        Qt::QueuedConnection);
                    updater->restartToApply();
                }
            });
        updater->downloadUpdate();
    }

    void MainWindow::openSettings()
    {
        if (processing_)
        {
            return;
        }
        SettingsDialog dialog(themeMode_,
            language_,
            checkForUpdatesOnStartup_,
            fileLogging_,
            gpuAcceleration_,
            videoQuality_,
            videoCodec_,
            this);

        connect(&dialog,
            &SettingsDialog::themeChanged,
            this,
            [this](ThemeMode mode)
            {
                themeMode_ = mode;
                applyTheme(*qApp, mode);
                updateSettingsIcon();
                saveSettings();
            });
        connect(&dialog,
            &SettingsDialog::languageChanged,
            this,
            [this](const QString &language)
            {
                if (language != language_)
                {
                    applyLanguage(language);
                    saveSettings();
                }
            });
        connect(&dialog,
            &SettingsDialog::checkForUpdatesChanged,
            this,
            [this](bool enabled)
            {
                checkForUpdatesOnStartup_ = enabled;
                saveSettings();
            });
        connect(&dialog,
            &SettingsDialog::fileLoggingChanged,
            this,
            [this](bool enabled)
            {
                fileLogging_ = enabled;
                saveSettings();
            });
        connect(&dialog,
            &SettingsDialog::videoQualityChanged,
            this,
            [this](int quality)
            {
                videoQuality_ = std::clamp(quality, 0, 2);
                saveSettings();
            });
        connect(&dialog,
            &SettingsDialog::videoCodecChanged,
            this,
            [this](int codec)
            {
                videoCodec_ = std::clamp(codec, 0, 1);
                saveSettings();
            });
        connect(&dialog,
            &SettingsDialog::gpuAccelerationChanged,
            this,
            [this](bool enabled)
            {
                if (gpuAcceleration_ != enabled)
                {
                    gpuAcceleration_ = enabled;
                    cachedDetector_.reset();
                    cachedVideoDetector_.reset();
                    cachedDetectorKey_ = {};
                    cachedVideoDetectorKey_ = {};
                    cachedPlateDetector_.reset();
                    cachedPlateDetectorKey_ = {};
                    saveSettings();
                }
            });

        dialog.exec();
    }

    void MainWindow::updateAnonymizationSample() const
    {
        if (samplePreview_ == nullptr)
        {
            return;
        }
        samplePreview_->setPixmap(anonymizationSamplePixmap(
            static_cast<AnonymizationMethod>(methodCombo_->currentData().toInt()),
            static_cast<MaskShape>(shapeCombo_->currentData().toInt()),
            blockSizeSpin_->value(),
            static_cast<float>(paddingSpin_->value()),
            softEdgeCheck_->isChecked(),
            customImage_));
    }

    void MainWindow::updateAnonymizationControls() const
    {
        if (methodCombo_ == nullptr)
        {
            return;
        }
        const auto method = static_cast<AnonymizationMethod>(methodCombo_->currentData().toInt());
        const bool customImage = method == AnonymizationMethod::CustomImage;
        const bool editable = !processing_;
        if (customImageLabel_ != nullptr)
        {
            customImageLabel_->setVisible(customImage);
        }
        if (customImagePicker_ != nullptr)
        {
            customImagePicker_->setVisible(customImage);
        }
        if (customImagePathEdit_ != nullptr)
        {
            customImagePathEdit_->setEnabled(editable && customImage);
        }
        if (customImageBrowseButton_ != nullptr)
        {
            customImageBrowseButton_->setEnabled(editable && customImage);
        }
        if (shapeCombo_ != nullptr)
        {
            shapeCombo_->setEnabled(editable && !customImage);
        }
        if (softEdgeCheck_ != nullptr)
        {
            softEdgeCheck_->setEnabled(editable && !customImage);
        }
        if (blockSizeSpin_ != nullptr)
        {
            blockSizeSpin_->setEnabled(editable && method == AnonymizationMethod::Mosaic);
        }
    }

    void MainWindow::updateSettingsIcon() const
    {
        if (settingsButton_ == nullptr)
        {
            return;
        }
        const QIcon icon = settingsGearIcon(themeMode_);
        if (icon.isNull())
        {
            settingsButton_->setText(QStringLiteral("⚙"));
        }
        else
        {
            settingsButton_->setText(QString());
            settingsButton_->setIcon(icon);
            settingsButton_->setIconSize(QSize(20, 20));
        }
    }

    void MainWindow::addInputPath(const QString &path) const
    {
        if (processing_ || path.isEmpty())
        {
            return;
        }

        const QFileInfo info(path);
        const QString canonical = info.canonicalFilePath();
        const QString key = canonical.isEmpty() ? QDir::cleanPath(path) : canonical;

        for (int i = 0; i < inputList_->count(); ++i)
        {
            const QString existing = inputList_->item(i)->text();
            const QFileInfo existingInfo(existing);
            const QString existingKey = existingInfo.canonicalFilePath().isEmpty()
                                            ? QDir::cleanPath(existing)
                                            : existingInfo.canonicalFilePath();
            if (existingKey.compare(key, Qt::CaseInsensitive) == 0)
            {
                return;
            }
        }

        auto *item = new QListWidgetItem(path);
        if (info.isDir())
        {
            item->setIcon(style()->standardIcon(QStyle::SP_DirIcon));
        }
        else if (isSupportedVideo(pathFromQString(path)))
        {
            const QIcon thumbnail = videoThumbnailIcon(path);
            item->setIcon(
                thumbnail.isNull() ? style()->standardIcon(QStyle::SP_FileIcon) : thumbnail);
        }
        else
        {
            QImageReader reader(path);
            reader.setAutoTransform(true);
            QSize thumbSize = reader.size();
            if (thumbSize.isValid())
            {
                thumbSize.scale(40, 40, Qt::KeepAspectRatio);
                reader.setScaledSize(thumbSize);
            }
            const QImage thumb = reader.read();
            item->setIcon(thumb.isNull() ? style()->standardIcon(QStyle::SP_FileIcon)
                                         : QIcon(QPixmap::fromImage(thumb)));
        }
        inputList_->addItem(item);
    }

    void MainWindow::setProcessing(bool processing)
    {
        processing_ = processing;
        setAcceptDrops(!processing);
        if (processing)
        {
            setDropHighlight(false);
        }

        if (statusLabel_->property("state").isValid())
        {
            statusLabel_->setProperty("state", QVariant());
            statusLabel_->style()->unpolish(statusLabel_);
            statusLabel_->style()->polish(statusLabel_);
        }

        startButton_->setEnabled(!processing);
        stopButton_->setEnabled(processing);
        settingsButton_->setEnabled(!processing);
        settingsShortcut_->setEnabled(!processing);
        detectCombo_->setEnabled(!processing);
        modelCombo_->setEnabled(!processing);
        methodCombo_->setEnabled(!processing);
        customImagePathEdit_->setEnabled(!processing);
        customImageBrowseButton_->setEnabled(!processing);
        shapeCombo_->setEnabled(!processing);
        softEdgeCheck_->setEnabled(!processing);
        modelPathEdit_->setEnabled(!processing);
        downloadButton_->setEnabled(!processing);
        modelBrowseButton_->setEnabled(!processing);
        outputDirEdit_->setEnabled(!processing);
        outputBrowseButton_->setEnabled(!processing);
        inputList_->setEnabled(!processing);
        addFilesButton_->setEnabled(!processing);
        addFolderButton_->setEnabled(!processing);
        clearInputsButton_->setEnabled(!processing);
        recursiveCheck_->setEnabled(!processing);
        reviewCheck_->setEnabled(!processing);
        preserveMetaCheck_->setEnabled(!processing && metadataSupportAvailable());
        scoreThresholdSpin_->setEnabled(!processing);
        nmsThresholdSpin_->setEnabled(!processing);
        blockSizeSpin_->setEnabled(!processing);
        paddingSpin_->setEnabled(!processing);
        advancedToggle_->setEnabled(!processing);
        resetAdvancedButton_->setEnabled(!processing);

        if (!processing)
        {
            updateAnonymizationControls();
            const bool facesNeeded = detectCombo_->currentData().toString() != "plates";
            modelCombo_->setEnabled(facesNeeded);
            modelPathEdit_->setEnabled(facesNeeded);
            modelBrowseButton_->setEnabled(facesNeeded);
            downloadButton_->setEnabled(facesNeeded);
        }
    }

    void MainWindow::setDropHighlight(bool active) const
    {
        inputList_->setProperty("dragActive", active ? QVariant(true) : QVariant());
        inputList_->style()->unpolish(inputList_);
        inputList_->style()->polish(inputList_);
    }

    QStringList MainWindow::inputPaths() const
    {
        QStringList paths;
        for (int i = 0; i < inputList_->count(); ++i)
        {
            paths.append(inputList_->item(i)->text());
        }
        return paths;
    }

    void MainWindow::populateBundledModels()
    {
        for (std::size_t index = 0; index < builtinModels().size(); ++index)
        {
            const auto &model = builtinModels()[index];
            const auto existing = firstExistingModelPath(model.fileName);
            const auto path =
                existing.isEmpty() ? modelCacheDir() + "/" + model.fileName : existing;
            modelCombo_->addItem(QString(), path);
            modelCombo_->setItemData(
                modelCombo_->count() - 1, static_cast<int>(index), kModelCatalogIndexRole);
        }

        addRetranslation(
            [this]
            {
                if (modelCombo_->count() >= 2)
                {
                    modelCombo_->setItemText(0, tr("Accurate  ·  YOLO5Face-n"));
                    modelCombo_->setItemText(1, tr("Fast  ·  YuNet"));
                }
            });

        modelCombo_->setCurrentIndex(0);
        updateModelPathFromSelection();
    }

    void MainWindow::downloadSelectedModel()
    {
        if (processing_)
        {
            return;
        }
        const auto modelPath = selectedModelPath();
        const BuiltinModel *builtin = selectedBuiltinModel();
        if (builtin == nullptr)
        {
            return;
        }
        if (QFileInfo::exists(modelPath))
        {
            updateModelPathFromSelection();
            return;
        }

        appendLog(tr("Downloading %1…").arg(builtin->fileName));
        if (ensureBuiltinModelAvailable(this, *builtin, modelPath))
        {
            appendLog(tr("Model ready: %1").arg(builtin->fileName));
        }
        else
        {
            appendLog(tr("Model download was cancelled or failed."));
        }
        updateModelPathFromSelection();
    }

    void MainWindow::updateModelPathFromSelection() const
    {
        const auto path = selectedModelPath();
        const bool exists = !path.isEmpty() && QFileInfo::exists(path);
        const bool missingBuiltin = !exists && selectedBuiltinModel() != nullptr;

        if (!path.isEmpty() && missingBuiltin)
        {
            modelPathEdit_->setText(tr("Not downloaded yet — click Download"));
        }
        else
        {
            modelPathEdit_->setText(path);
        }

        if (downloadButton_ != nullptr)
        {
            downloadButton_->setVisible(missingBuiltin);
        }
    }

    QString MainWindow::selectedModelPath() const
    {
        if (modelCombo_ == nullptr || modelCombo_->currentIndex() < 0)
        {
            return {};
        }

        return modelCombo_->currentData().toString();
    }

    const BuiltinModel *MainWindow::selectedBuiltinModel() const
    {
        if (modelCombo_ == nullptr || modelCombo_->currentIndex() < 0)
        {
            return nullptr;
        }
        bool validIndex = false;
        const int index = modelCombo_->currentData(kModelCatalogIndexRole).toInt(&validIndex);
        if (!validIndex || index < 0 || index >= static_cast<int>(builtinModels().size()))
        {
            return nullptr;
        }
        return &builtinModels()[static_cast<std::size_t>(index)];
    }

    bool MainWindow::ensureCustomModelStillApproved(const QString &path)
    {
        switch (checkCustomModel(path, customModelApproval_))
        {
        case CustomModelState::Approved:
            return true;
        case CustomModelState::Unavailable:
            appendLog(tr("Could not read the custom model file."));
            return false;
        case CustomModelState::Unapproved:
            break;
        }

        // Never approved as content - a model chosen by an older version - versus approved and
        // then replaced. The second is the one worth alarming about.
        const bool approved = customModelApproval_.isRecorded()
                                  ? confirmChangedCustomModel(this, path)
                                  : confirmTrustedCustomModel(this, path);
        if (!approved)
        {
            appendLog(tr("The custom model was not approved, so nothing was processed."));
            return false;
        }
        const auto approval = approvalForCustomModel(path);
        if (!approval)
        {
            appendLog(tr("Could not read the custom model file."));
            return false;
        }
        customModelApproval_ = *approval;
        return true;
    }

    void MainWindow::addRetranslation(std::function<void()> apply)
    {
        apply();
        retranslators_.push_back(std::move(apply));
    }

    void MainWindow::retranslateUi()
    {
        for (const auto &apply : retranslators_)
        {
            apply();
        }
    }

    void MainWindow::applyLanguage(const QString &language)
    {
        qApp->removeTranslator(&translator_);
        qApp->removeTranslator(&qtTranslator_);
        if (language != "en")
        {
            if (translator_.load(":/i18n/cloakframe_" + language + ".qm"))
            {
                qApp->installTranslator(&translator_);
            }
            if (qtTranslator_.load(
                    "qtbase_" + language, QLibraryInfo::path(QLibraryInfo::TranslationsPath)))
            {
                qApp->installTranslator(&qtTranslator_);
            }
        }
        language_ = language;
    }

    void MainWindow::changeEvent(QEvent *event)
    {
        if (event->type() == QEvent::LanguageChange)
        {
            retranslateUi();
        }
        QMainWindow::changeEvent(event);
    }

    void MainWindow::appendLog(const QString &message) const
    {
        const auto time = QDateTime::currentDateTime().toString("HH:mm:ss");
        logEdit_->appendPlainText(QString("[%1]  %2").arg(time, message));
        spdlog::info("{}", message.toStdString());
    }

    void MainWindow::reportValidationIssue(const QString &message, QWidget *field) const
    {
        statusLabel_->setProperty("state", "warning");
        statusLabel_->style()->unpolish(statusLabel_);
        statusLabel_->style()->polish(statusLabel_);
        statusLabel_->setText(QStringLiteral("⚠  ") + message);
        appendLog(message);

        if (field != nullptr)
        {
            field->setProperty("invalid", true);
            field->style()->unpolish(field);
            field->style()->polish(field);
            QTimer::singleShot(2200,
                field,
                [field]
                {
                    field->setProperty("invalid", QVariant());
                    field->style()->unpolish(field);
                    field->style()->polish(field);
                });
            field->setFocus(Qt::OtherFocusReason);
        }
    }

    ReviewResult MainWindow::requestReview(const QImage &image,
        const QString &sourceName,
        const QVector<QRectF> &detected,
        int currentIndex,
        int total,
        double previewScale)
    {
        if (shuttingDown_)
        {
            ReviewResult cancelAll;
            cancelAll.decision = ReviewDecision::CancelAll;
            return cancelAll;
        }
        ReviewPreviewSpec spec;
        bool preserveMetadata = false;
        if (activeRunState_.has_value())
        {
            spec.method = activeRunState_->method;
            spec.customImage = activeRunState_->customImage;
            spec.shape = activeRunState_->shape;
            spec.softEdges = activeRunState_->softEdges;
            spec.blockSize = activeRunState_->blockSize;
            spec.padding = activeRunState_->padding;
            preserveMetadata = activeRunState_->preserveMetadata;
        }
        else
        {
            spec.method = static_cast<AnonymizationMethod>(methodCombo_->currentData().toInt());
            spec.customImage = customImage_;
            spec.shape = static_cast<MaskShape>(shapeCombo_->currentData().toInt());
            spec.softEdges = softEdgeCheck_ != nullptr && softEdgeCheck_->isChecked();
            spec.blockSize = blockSizeSpin_->value();
            spec.padding = static_cast<float>(paddingSpin_->value());
            preserveMetadata = preserveMetaCheck_ != nullptr && preserveMetaCheck_->isChecked();
        }
        spec.previewScale = previewScale;
        ReviewDialog dialog(
            image, sourceName, detected, currentIndex, total, preserveMetadata, spec, this);
        dialog.exec();
        return dialog.reviewResult();
    }

    VideoReviewResult MainWindow::requestVideoReview(const VideoReviewRequest &request)
    {
        if (shuttingDown_)
        {
            VideoReviewResult cancelAll;
            cancelAll.decision = VideoReviewDecision::CancelAll;
            return cancelAll;
        }
        VideoReviewDialog dialog(request, this);
        dialog.exec();
        return dialog.reviewResult();
    }
}
