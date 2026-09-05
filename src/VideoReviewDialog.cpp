#include "cloakframe/VideoReviewDialog.hpp"

#include "cloakframe/ReviewTypes.hpp"

#include <QCoreApplication>
#include <QDialogButtonBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QProcess>
#include <QPushButton>
#include <QShortcut>
#include <QSignalBlocker>
#include <QSlider>
#include <QStyle>
#include <QStyleOptionSlider>
#include <QTimer>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>

namespace cloakframe
{
    namespace
    {
        constexpr int kTrackIdRole = Qt::UserRole;
        constexpr int kManualTrackRole = Qt::UserRole + 1;

        const VideoReviewBox *boxAtFrame(const VideoReviewTrack &track, int frame)
        {
            const auto it = std::lower_bound(track.boxes.cbegin(),
                track.boxes.cend(),
                frame,
                [](const VideoReviewBox &box, int value)
                {
                    return box.frame < value;
                });
            return it != track.boxes.cend() && it->frame == frame ? &*it : nullptr;
        }

        QString formatTime(double seconds)
        {
            seconds = std::max(0.0, seconds);
            const int total = static_cast<int>(std::floor(seconds));
            const int hours = total / 3600;
            const int minutes = (total / 60) % 60;
            const int secs = total % 60;
            return hours > 0
                       ? QStringLiteral("%1:%2:%3")
                             .arg(hours)
                             .arg(minutes, 2, 10, QLatin1Char('0'))
                             .arg(secs, 2, 10, QLatin1Char('0'))
                       : QStringLiteral("%1:%2").arg(minutes).arg(secs, 2, 10, QLatin1Char('0'));
        }

        QString frameTime(const VideoReviewRequest &request, int frame)
        {
            const double fps = request.fpsNum > 0 && request.fpsDen > 0
                                   ? static_cast<double>(request.fpsNum) / request.fpsDen
                                   : request.fps;
            const qint64 milliseconds = fps > 0.0 ? std::llround(frame * 1000.0 / fps) : 0;
            return QStringLiteral("%1.%2")
                .arg(formatTime(static_cast<double>(milliseconds) / 1000.0))
                .arg(milliseconds % 1000, 3, 10, QLatin1Char('0'));
        }

    }

    class VideoReviewCanvas final : public QWidget
    {
    public:
        explicit VideoReviewCanvas(QWidget *parent = nullptr)
            : QWidget(parent)
        {
            setMinimumSize(640, 360);
            setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
        }

        void setData(const VideoReviewRequest *request,
            const QSet<int> *excluded,
            const QVector<VideoReviewManualTrack> *manualTracks)
        {
            request_ = request;
            excluded_ = excluded;
            manualTracks_ = manualTracks;
        }

        void setFrame(int frame, QImage image)
        {
            drawing_ = false;
            frame_ = frame;
            image_ = std::move(image);
            loading_ = false;
            update();
        }

        void setLoadingFrame(int frame)
        {
            drawing_ = false;
            frame_ = frame;
            image_ = {};
            loading_ = true;
            update();
        }

        void setToggleCallback(std::function<void(int)> callback)
        {
            toggleTrack_ = std::move(callback);
        }

        void setManualTrackSelectedCallback(std::function<void(int)> callback)
        {
            selectManualTrack_ = std::move(callback);
        }

        void setManualBoxCallback(std::function<void(int, const QRectF &)> callback)
        {
            manualBox_ = std::move(callback);
        }

        void setSelectedManualTrack(int id)
        {
            selectedManualTrackId_ = id;
            update();
        }

        void setDrawingMode(bool enabled)
        {
            drawingMode_ = enabled;
            drawing_ = false;
            setCursor(enabled ? Qt::CrossCursor : Qt::ArrowCursor);
            update();
        }

        void refresh()
        {
            update();
        }

    protected:
        void paintEvent(QPaintEvent *) override
        {
            QPainter painter(this);
            painter.setRenderHint(QPainter::Antialiasing, true);
            painter.fillRect(rect(), QColor("#111827"));
            if (request_ == nullptr || image_.isNull())
            {
                painter.setPen(QColor("#D1D5DB"));
                painter.drawText(rect(),
                    Qt::AlignCenter,
                    loading_ ? QCoreApplication::translate(
                                   "cloakframe::VideoReviewCanvas", "Loading frame preview…")
                             : QCoreApplication::translate("cloakframe::VideoReviewCanvas",
                                   "Could not load this frame preview."));
                return;
            }

            const QSizeF fitted = image_.size().scaled(size(), Qt::KeepAspectRatio);
            const QRectF target((width() - fitted.width()) * 0.5,
                (height() - fitted.height()) * 0.5,
                fitted.width(),
                fitted.height());
            painter.drawImage(target, image_);

            const double sx = target.width() / request_->frameSize.width();
            const double sy = target.height() / request_->frameSize.height();
            for (const auto &track : request_->tracks)
            {
                const auto *box = boxAtFrame(track, frame_);
                if (box == nullptr)
                {
                    continue;
                }
                const QRectF screen(target.x() + box->rect.x() * sx,
                    target.y() + box->rect.y() * sy,
                    box->rect.width() * sx,
                    box->rect.height() * sy);
                const bool excluded = excluded_ != nullptr && excluded_->contains(track.id);
                QColor color = excluded ? QColor("#9CA3AF") : QColor("#F59E0B");
                painter.setPen(
                    QPen(color, excluded ? 2.0 : 3.0, excluded ? Qt::DashLine : Qt::SolidLine));
                QColor fill = color;
                fill.setAlpha(excluded ? 28 : 52);
                painter.fillRect(screen, fill);
                painter.drawRect(screen);
                painter.setPen(Qt::white);
                painter.drawText(screen.adjusted(4, 2, -4, -2),
                    Qt::AlignLeft | Qt::AlignTop,
                    QCoreApplication::translate("cloakframe::VideoReviewCanvas", "Track %1")
                        .arg(track.id));
            }

            if (manualTracks_ != nullptr)
            {
                for (const auto &track : *manualTracks_)
                {
                    const auto box = manualTrackRectAtFrame(track, frame_);
                    if (!box)
                    {
                        continue;
                    }
                    const QRectF screen(target.x() + box->x() * sx,
                        target.y() + box->y() * sy,
                        box->width() * sx,
                        box->height() * sy);
                    const bool selected = track.id == selectedManualTrackId_;
                    const QColor color = selected ? QColor("#EC4899") : QColor("#22D3EE");
                    painter.setPen(QPen(color, selected ? 3.0 : 2.5));
                    QColor fill = color;
                    fill.setAlpha(selected ? 58 : 42);
                    painter.fillRect(screen, fill);
                    painter.drawRect(screen);
                    painter.setPen(Qt::white);
                    painter.drawText(screen.adjusted(4, 2, -4, -2),
                        Qt::AlignLeft | Qt::AlignTop,
                        QCoreApplication::translate("cloakframe::VideoReviewCanvas", "Manual %1")
                            .arg(track.id));
                }
            }

            if (drawing_)
            {
                const QRectF drawn(dragStart_, dragCurrent_);
                painter.setPen(QPen(QColor("#EC4899"), 2.0, Qt::DashLine));
                painter.fillRect(drawn.normalized(), QColor(236, 72, 153, 38));
                painter.drawRect(drawn.normalized());
            }
        }

        void mousePressEvent(QMouseEvent *event) override
        {
            if (request_ == nullptr || image_.isNull() || event->button() != Qt::LeftButton)
            {
                return;
            }
            const QRectF target = imageTarget();
            if (!target.contains(event->position()))
            {
                return;
            }

            if (drawingMode_)
            {
                drawing_ = true;
                dragStart_ = event->position();
                dragCurrent_ = event->position();
                update();
                return;
            }

            const double sx = target.width() / request_->frameSize.width();
            const double sy = target.height() / request_->frameSize.height();
            if (manualTracks_ != nullptr && selectManualTrack_)
            {
                for (auto it = manualTracks_->crbegin(); it != manualTracks_->crend(); ++it)
                {
                    const auto box = manualTrackRectAtFrame(*it, frame_);
                    if (!box)
                    {
                        continue;
                    }
                    const QRectF screen(target.x() + box->x() * sx,
                        target.y() + box->y() * sy,
                        box->width() * sx,
                        box->height() * sy);
                    if (screen.contains(event->position()))
                    {
                        selectManualTrack_(it->id);
                        return;
                    }
                }
            }
            if (!toggleTrack_)
            {
                return;
            }
            for (auto it = request_->tracks.crbegin(); it != request_->tracks.crend(); ++it)
            {
                const auto *box = boxAtFrame(*it, frame_);
                if (box == nullptr)
                {
                    continue;
                }
                const QRectF screen(target.x() + box->rect.x() * sx,
                    target.y() + box->rect.y() * sy,
                    box->rect.width() * sx,
                    box->rect.height() * sy);
                if (screen.contains(event->position()))
                {
                    toggleTrack_(it->id);
                    return;
                }
            }
        }

        void mouseMoveEvent(QMouseEvent *event) override
        {
            if (!drawing_)
            {
                return;
            }
            const QRectF target = imageTarget();
            dragCurrent_.setX(std::clamp(event->position().x(), target.left(), target.right()));
            dragCurrent_.setY(std::clamp(event->position().y(), target.top(), target.bottom()));
            update();
        }

        void mouseReleaseEvent(QMouseEvent *event) override
        {
            if (!drawing_ || event->button() != Qt::LeftButton)
            {
                return;
            }
            mouseMoveEvent(event);
            drawing_ = false;
            const QRectF screenRect(dragStart_, dragCurrent_);
            const QRectF normalized = screenRect.normalized();
            if (normalized.width() >= 4.0 && normalized.height() >= 4.0 && manualBox_)
            {
                const QRectF target = imageTarget();
                const double sx = target.width() / request_->frameSize.width();
                const double sy = target.height() / request_->frameSize.height();
                const QRectF frameRect((normalized.x() - target.x()) / sx,
                    (normalized.y() - target.y()) / sy,
                    normalized.width() / sx,
                    normalized.height() / sy);
                manualBox_(frame_, frameRect);
            }
            update();
        }

    private:
        [[nodiscard]] QRectF imageTarget() const
        {
            const QSizeF fitted = image_.size().scaled(size(), Qt::KeepAspectRatio);
            return {(width() - fitted.width()) * 0.5,
                (height() - fitted.height()) * 0.5,
                fitted.width(),
                fitted.height()};
        }

        const VideoReviewRequest *request_ = nullptr;
        const QSet<int> *excluded_ = nullptr;
        const QVector<VideoReviewManualTrack> *manualTracks_ = nullptr;
        QImage image_;
        int frame_ = 0;
        int selectedManualTrackId_ = 0;
        bool drawingMode_ = false;
        bool drawing_ = false;
        bool loading_ = false;
        QPointF dragStart_;
        QPointF dragCurrent_;
        std::function<void(int)> toggleTrack_;
        std::function<void(int)> selectManualTrack_;
        std::function<void(int, const QRectF &)> manualBox_;
    };

    class VideoTimeline final : public QSlider
    {
    public:
        explicit VideoTimeline(QWidget *parent = nullptr)
            : QSlider(Qt::Horizontal, parent)
        {
            setMinimumHeight(34);
        }

        void setData(const VideoReviewRequest *request,
            const QSet<int> *excluded,
            const QVector<VideoReviewManualTrack> *manualTracks)
        {
            request_ = request;
            excluded_ = excluded;
            manualTracks_ = manualTracks;
            update();
        }

    protected:
        void paintEvent(QPaintEvent *event) override
        {
            QSlider::paintEvent(event);
            if (request_ == nullptr || request_->frameCount < 2)
            {
                return;
            }
            QStyleOptionSlider option;
            initStyleOption(&option);
            const QRect groove =
                style()->subControlRect(QStyle::CC_Slider, &option, QStyle::SC_SliderGroove, this);
            QPainter painter(this);
            painter.setRenderHint(QPainter::Antialiasing, false);
            const int y = height() - 5;
            for (const auto &track : request_->tracks)
            {
                if (track.boxes.isEmpty())
                {
                    continue;
                }
                const double denominator = static_cast<double>(request_->frameCount - 1);
                const int x1 = groove.left()
                               + static_cast<int>(std::lround(
                                   track.boxes.front().frame / denominator * groove.width()));
                const int x2 = groove.left()
                               + static_cast<int>(std::lround(
                                   track.boxes.back().frame / denominator * groove.width()));
                const bool excluded = excluded_ != nullptr && excluded_->contains(track.id);
                painter.fillRect(QRect(x1, y, std::max(2, x2 - x1 + 1), 3),
                    excluded ? QColor("#9CA3AF") : QColor("#F59E0B"));
            }
            if (manualTracks_ != nullptr)
            {
                for (const auto &track : *manualTracks_)
                {
                    const double denominator = static_cast<double>(request_->frameCount - 1);
                    const int x1 = groove.left()
                                   + static_cast<int>(std::lround(
                                       track.startFrame / denominator * groove.width()));
                    const int x2 = groove.left()
                                   + static_cast<int>(
                                       std::lround(track.endFrame / denominator * groove.width()));
                    painter.fillRect(
                        QRect(x1, y - 4, std::max(2, x2 - x1 + 1), 3), QColor("#22D3EE"));
                }
            }
            // Drawn on their own row above the tracks: these are the frames no track
            // covers, so they never coincide with a track bar worth overlaying.
            for (const auto &span : request_->uncoveredSpans)
            {
                const double denominator = static_cast<double>(request_->frameCount - 1);
                const int x1 =
                    groove.left()
                    + static_cast<int>(std::lround(span.firstFrame / denominator * groove.width()));
                const int x2 =
                    groove.left()
                    + static_cast<int>(std::lround(span.lastFrame / denominator * groove.width()));
                painter.fillRect(QRect(x1, y - 8, std::max(2, x2 - x1 + 1), 3), QColor("#EF4444"));
            }
        }

    private:
        const VideoReviewRequest *request_ = nullptr;
        const QSet<int> *excluded_ = nullptr;
        const QVector<VideoReviewManualTrack> *manualTracks_ = nullptr;
    };

    VideoReviewDialog::VideoReviewDialog(VideoReviewRequest request, QWidget *parent)
        : QDialog(parent)
        , request_(std::move(request))
    {
        for (const auto &track : request_.tracks)
        {
            if (track.id >= nextManualTrackId_ && track.id < std::numeric_limits<int>::max())
            {
                nextManualTrackId_ = track.id + 1;
            }
        }
        setWindowTitle(tr("Review video tracks — %1").arg(request_.sourceName));
        resize(1120, 720);
        setModal(true);

        auto *root = new QVBoxLayout(this);
        QString hintText =
            tr("Scrub the timeline and uncheck false detections. To cover a missed region, "
               "draw a manual track and add keyframes as it moves; the boxes between "
               "keyframes are interpolated before encoding.");
        const bool hasLowConfidenceTracks = std::any_of(request_.tracks.cbegin(),
            request_.tracks.cend(),
            [](const VideoReviewTrack &track)
            {
                return track.lowConfidence;
            });
        if (hasLowConfidenceTracks)
        {
            hintText += QStringLiteral(" ");
            hintText += tr("Tracks marked \"low confidence\" had too few confident "
                           "detections and are included by default — uncheck only false "
                           "detections.");
        }
        if (!request_.uncoveredSpans.isEmpty())
        {
            hintText += QStringLiteral(" ");
            hintText += tr("The red marks show gaps found before review where tracking could not "
                           "locate the subject. Add manual masks where needed. These marks remain "
                           "after editing because adding a mask does not verify the subject's "
                           "position.");
        }
        auto *hint = new QLabel(hintText, this);
        hint->setWordWrap(true);
        root->addWidget(hint);

        auto *body = new QHBoxLayout();
        canvas_ = new VideoReviewCanvas(this);
        canvas_->setData(&request_, &excludedTrackIds_, &manualTracks_);
        canvas_->setToggleCallback(
            [this](int id)
            {
                setTrackIncluded(id, excludedTrackIds_.contains(id));
            });
        canvas_->setManualTrackSelectedCallback(
            [this](int id)
            {
                selectManualTrack(id);
            });
        canvas_->setManualBoxCallback(
            [this](int frame, const QRectF &rect)
            {
                addOrUpdateManualKeyframe(frame, rect);
            });
        body->addWidget(canvas_, 1);

        auto *side = new QVBoxLayout();
        summaryLabel_ = new QLabel(this);
        side->addWidget(summaryLabel_);
        trackList_ = new QListWidget(this);
        trackList_->setObjectName("videoTracks");
        trackList_->setMinimumWidth(260);
        for (const auto &track : request_.tracks)
        {
            if (track.boxes.isEmpty())
            {
                continue;
            }
            const QString range = tr("Track %1  ·  %2–%3")
                                      .arg(track.id)
                                      .arg(formatTime(track.boxes.front().frame / request_.fps))
                                      .arg(formatTime(track.boxes.back().frame / request_.fps));
            auto *item = new QListWidgetItem(
                track.lowConfidence ? tr("%1  ·  low confidence").arg(range) : range, trackList_);
            item->setData(kTrackIdRole, track.id);
            item->setData(kManualTrackRole, false);
            item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
            if (track.lowConfidence)
            {
                item->setToolTip(tr("Few confident detections — included by default. Uncheck it to "
                                    "leave this track unredacted."));
            }
            item->setCheckState(Qt::Checked);
        }
        connect(trackList_,
            &QListWidget::itemChanged,
            this,
            [this](QListWidgetItem *item)
            {
                if (item->data(kManualTrackRole).toBool())
                {
                    return;
                }
                setTrackIncluded(
                    item->data(kTrackIdRole).toInt(), item->checkState() == Qt::Checked);
            });
        connect(trackList_,
            &QListWidget::currentItemChanged,
            this,
            [this](QListWidgetItem *current)
            {
                selectManualTrack(current != nullptr && current->data(kManualTrackRole).toBool()
                                      ? current->data(kTrackIdRole).toInt()
                                      : 0);
            });
        connect(trackList_,
            &QListWidget::itemActivated,
            this,
            [this](QListWidgetItem *item)
            {
                const int id = item->data(kTrackIdRole).toInt();
                if (item->data(kManualTrackRole).toBool())
                {
                    const auto *manual = manualTrack(id);
                    if (manual != nullptr)
                    {
                        timeline_->setValue(manual->startFrame);
                    }
                    return;
                }
                const auto it = std::find_if(request_.tracks.cbegin(),
                    request_.tracks.cend(),
                    [id](const VideoReviewTrack &track)
                    {
                        return track.id == id;
                    });
                if (it != request_.tracks.cend() && !it->boxes.isEmpty())
                {
                    timeline_->setValue(it->boxes.front().frame);
                }
            });
        side->addWidget(trackList_, 1);

        auto *gapLabel = new QLabel(tr("Tracking gaps (before review)"), this);
        side->addWidget(gapLabel);
        gapList_ = new QListWidget(this);
        gapList_->setObjectName("trackingGaps");
        gapList_->setAccessibleName(gapLabel->text());
        gapList_->setMaximumHeight(100);
        auto gaps = request_.uncoveredSpans;
        std::stable_sort(gaps.begin(),
            gaps.end(),
            [](const UncoveredSpan &a, const UncoveredSpan &b)
            {
                return a.firstFrame < b.firstFrame;
            });
        for (const auto &gap : gaps)
        {
            if (gap.firstFrame < 0 || gap.lastFrame < gap.firstFrame
                || gap.lastFrame >= request_.frameCount)
            {
                continue;
            }
            const QString text =
                tr("Track %1 · %2–%3 · Frames %4–%5")
                    .arg(gap.trackId)
                    .arg(frameTime(request_, gap.firstFrame), frameTime(request_, gap.lastFrame))
                    .arg(gap.firstFrame + 1)
                    .arg(gap.lastFrame + 1);
            auto *item = new QListWidgetItem(text, gapList_);
            item->setData(Qt::UserRole, gap.firstFrame);
            item->setToolTip(text);
        }
        side->addWidget(gapList_);
        auto *noGaps = new QLabel(tr("No tracking gaps were reported."), this);
        noGaps->setWordWrap(true);
        noGaps->setVisible(gapList_->count() == 0);
        gapList_->setVisible(gapList_->count() != 0);
        side->addWidget(noGaps);
        auto *gapButtons = new QHBoxLayout();
        previousGapButton_ = new QPushButton(tr("Previous gap"), this);
        nextGapButton_ = new QPushButton(tr("Next gap"), this);
        previousGapButton_->setObjectName("previousGap");
        nextGapButton_->setObjectName("nextGap");
        previousGapButton_->setAutoDefault(false);
        nextGapButton_->setAutoDefault(false);
        gapButtons->addWidget(previousGapButton_);
        gapButtons->addWidget(nextGapButton_);
        side->addLayout(gapButtons);
        connect(previousGapButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                navigateGap(false);
            });
        connect(nextGapButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                navigateGap(true);
            });
        connect(gapList_,
            &QListWidget::currentRowChanged,
            this,
            [this](int row)
            {
                if (row >= 0)
                {
                    timeline_->setValue(gapList_->item(row)->data(Qt::UserRole).toInt());
                }
            });
        connect(gapList_,
            &QListWidget::itemClicked,
            this,
            [this](QListWidgetItem *item)
            {
                timeline_->setValue(item->data(Qt::UserRole).toInt());
            });

        addManualTrackButton_ = new QPushButton(tr("Add missed track"), this);
        addKeyframeButton_ = new QPushButton(tr("Add / update keyframe"), this);
        setStartButton_ = new QPushButton(tr("Set start here"), this);
        setEndButton_ = new QPushButton(tr("Set end here"), this);
        removeManualTrackButton_ = new QPushButton(tr("Remove manual track"), this);
        side->addWidget(addManualTrackButton_);
        side->addWidget(addKeyframeButton_);
        auto *manualRangeButtons = new QHBoxLayout();
        manualRangeButtons->addWidget(setStartButton_);
        manualRangeButtons->addWidget(setEndButton_);
        side->addLayout(manualRangeButtons);
        side->addWidget(removeManualTrackButton_);
        drawStatusLabel_ = new QLabel(this);
        drawStatusLabel_->setWordWrap(true);
        drawStatusLabel_->setVisible(false);
        side->addWidget(drawStatusLabel_);
        connect(addManualTrackButton_,
            &QPushButton::clicked,
            this,
            &VideoReviewDialog::startNewManualTrack);
        connect(addKeyframeButton_,
            &QPushButton::clicked,
            this,
            &VideoReviewDialog::startManualKeyframe);
        connect(setStartButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                setManualTrackBoundary(true);
            });
        connect(setEndButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                setManualTrackBoundary(false);
            });
        connect(removeManualTrackButton_,
            &QPushButton::clicked,
            this,
            &VideoReviewDialog::removeSelectedManualTrack);

        auto *selectionButtons = new QHBoxLayout();
        auto *includeAll = new QPushButton(tr("Include all"), this);
        auto *excludeAll = new QPushButton(tr("Exclude all"), this);
        selectionButtons->addWidget(includeAll);
        selectionButtons->addWidget(excludeAll);
        side->addLayout(selectionButtons);
        connect(includeAll,
            &QPushButton::clicked,
            this,
            [this]
            {
                const QSignalBlocker blocker(trackList_);
                excludedTrackIds_.clear();
                for (int i = 0; i < trackList_->count(); ++i)
                {
                    auto *item = trackList_->item(i);
                    if (!item->data(kManualTrackRole).toBool())
                    {
                        item->setCheckState(Qt::Checked);
                    }
                }
                canvas_->refresh();
                timeline_->update();
                updateSummary();
            });
        connect(excludeAll,
            &QPushButton::clicked,
            this,
            [this]
            {
                const QSignalBlocker blocker(trackList_);
                for (const auto &track : request_.tracks)
                {
                    excludedTrackIds_.insert(track.id);
                }
                for (int i = 0; i < trackList_->count(); ++i)
                {
                    auto *item = trackList_->item(i);
                    if (!item->data(kManualTrackRole).toBool())
                    {
                        item->setCheckState(Qt::Unchecked);
                    }
                }
                canvas_->refresh();
                timeline_->update();
                updateSummary();
            });
        body->addLayout(side);
        root->addLayout(body, 1);

        auto *timeRow = new QHBoxLayout();
        timeline_ = new VideoTimeline(this);
        timeline_->setObjectName("videoTimeline");
        timeline_->setRange(0, std::max(0, request_.frameCount - 1));
        timeline_->setPageStep(std::max(1, static_cast<int>(std::lround(request_.fps))));
        timeline_->setData(&request_, &excludedTrackIds_, &manualTracks_);
        timeLabel_ = new QLabel(this);
        timeLabel_->setObjectName("videoFramePosition");
        timeLabel_->setMinimumWidth(120);
        previousFrameButton_ = new QPushButton(QStringLiteral("‹"), this);
        nextFrameButton_ = new QPushButton(QStringLiteral("›"), this);
        previousFrameButton_->setObjectName("previousFrame");
        nextFrameButton_->setObjectName("nextFrame");
        previousFrameButton_->setToolTip(tr("Previous frame (Left arrow)"));
        nextFrameButton_->setToolTip(tr("Next frame (Right arrow)"));
        for (auto *button : {previousFrameButton_, nextFrameButton_})
        {
            button->setAccessibleName(button->toolTip());
            button->setAutoDefault(false);
            button->setMaximumWidth(40);
            timeRow->addWidget(button);
        }
        connect(previousFrameButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                timeline_->setValue(std::max(0, currentFrame_ - 1));
            });
        connect(nextFrameButton_,
            &QPushButton::clicked,
            this,
            [this]
            {
                timeline_->setValue(std::min(timeline_->maximum(), currentFrame_ + 1));
            });
        auto *previousFrameShortcut = new QShortcut(QKeySequence(Qt::Key_Left), this);
        auto *nextFrameShortcut = new QShortcut(QKeySequence(Qt::Key_Right), this);
        connect(previousFrameShortcut,
            &QShortcut::activated,
            previousFrameButton_,
            &QPushButton::click);
        connect(nextFrameShortcut, &QShortcut::activated, nextFrameButton_, &QPushButton::click);
        timeRow->addWidget(timeline_, 1);
        timeRow->addWidget(timeLabel_);
        root->addLayout(timeRow);

        seekTimer_ = new QTimer(this);
        seekTimer_->setSingleShot(true);
        seekTimer_->setInterval(120);
        connect(seekTimer_, &QTimer::timeout, this, &VideoReviewDialog::loadFramePreview);
        connect(timeline_, &QSlider::valueChanged, this, &VideoReviewDialog::setFrame);

        auto *buttons = new QDialogButtonBox(this);
        auto *cancel = buttons->addButton(tr("Cancel all"), QDialogButtonBox::RejectRole);
        auto *encode = buttons->addButton(tr("Encode video"), QDialogButtonBox::AcceptRole);
        connect(cancel, &QPushButton::clicked, this, &VideoReviewDialog::reject);
        connect(encode,
            &QPushButton::clicked,
            this,
            [this]
            {
                const qsizetype remaining =
                    request_.tracks.size() - excludedTrackIds_.size() + manualTracks_.size();
                if (reviewClearedEveryDetection(request_.tracks.size(), remaining))
                {
                    const auto answer = QMessageBox::warning(this,
                        tr("Encode with nothing covered?"),
                        tr("Every track the detector found has been excluded and none were "
                           "added in their place, so this video will be encoded with nothing "
                           "covered.\n\nContinue?"),
                        QMessageBox::Yes | QMessageBox::No,
                        QMessageBox::No);
                    if (answer != QMessageBox::Yes)
                    {
                        return;
                    }
                }
                accept();
            });
        root->addWidget(buttons);

        updateSummary();
        selectManualTrack(0);
        int initialFrame = 0;
        if (!request_.tracks.isEmpty() && !request_.tracks.front().boxes.isEmpty())
        {
            initialFrame = request_.tracks.front().boxes.front().frame;
        }
        timeline_->setValue(initialFrame);
        setFrame(initialFrame);
        loadFramePreview();
    }

    VideoReviewDialog::~VideoReviewDialog()
    {
        cancelFramePreview();
    }

    VideoReviewResult VideoReviewDialog::reviewResult() const
    {
        VideoReviewResult result;
        result.decision = decision_;
        result.excludedTrackIds = excludedTrackIds_.values();
        std::sort(result.excludedTrackIds.begin(), result.excludedTrackIds.end());
        result.addedTracks = manualTracks_;
        return result;
    }

    void VideoReviewDialog::reject()
    {
        decision_ = VideoReviewDecision::CancelAll;
        QDialog::reject();
    }

    void VideoReviewDialog::setFrame(int frame)
    {
        currentFrame_ = std::clamp(frame, 0, std::max(0, request_.frameCount - 1));
        timeLabel_->setText(tr("%1 / %2 · Frame %3 / %4")
                .arg(frameTime(request_, currentFrame_),
                    frameTime(request_, std::max(0, request_.frameCount - 1)))
                .arg(request_.frameCount > 0 ? currentFrame_ + 1 : 0)
                .arg(request_.frameCount));
        updateNavigation();
        if (frameCache_.contains(currentFrame_))
        {
            seekTimer_->stop();
            cancelFramePreview();
            frameCacheOrder_.removeAll(currentFrame_);
            frameCacheOrder_.push_back(currentFrame_);
            canvas_->setFrame(currentFrame_, frameCache_.value(currentFrame_));
        }
        else
        {
            cancelFramePreview();
            canvas_->setLoadingFrame(currentFrame_);
            seekTimer_->start();
        }
    }

    void VideoReviewDialog::navigateGap(bool next)
    {
        for (int offset = 0; offset < gapList_->count(); ++offset)
        {
            const int row = next ? offset : gapList_->count() - 1 - offset;
            const int frame = gapList_->item(row)->data(Qt::UserRole).toInt();
            if (next ? frame > currentFrame_ : frame < currentFrame_)
            {
                timeline_->setValue(frame);
                return;
            }
        }
    }

    void VideoReviewDialog::updateNavigation()
    {
        previousFrameButton_->setEnabled(currentFrame_ > 0);
        nextFrameButton_->setEnabled(currentFrame_ < request_.frameCount - 1);
        bool previous = false;
        bool next = false;
        int selected = -1;
        for (int row = 0; row < gapList_->count(); ++row)
        {
            const int frame = gapList_->item(row)->data(Qt::UserRole).toInt();
            previous |= frame < currentFrame_;
            next |= frame > currentFrame_;
            if (frame == currentFrame_ && (selected < 0 || row == gapList_->currentRow()))
            {
                selected = row;
            }
        }
        previousGapButton_->setEnabled(previous);
        nextGapButton_->setEnabled(next);
        const QSignalBlocker blocker(gapList_);
        gapList_->setCurrentRow(selected);
    }

    void VideoReviewDialog::loadFramePreview()
    {
        seekTimer_->stop();
        const int requestedFrame = currentFrame_;
        const QStringList arguments = videoPreviewFrameArguments(request_, requestedFrame);
        if (arguments.isEmpty())
        {
            canvas_->setFrame(requestedFrame, {});
            return;
        }

        cancelFramePreview();
        const quint64 generation = previewGeneration_;
        auto *process = new QProcess(this);
        previewProcess_ = process;
        process->setProcessChannelMode(QProcess::SeparateChannels);
        connect(process,
            &QProcess::finished,
            this,
            [this, process, requestedFrame, generation](
                const int exitCode, const QProcess::ExitStatus exitStatus)
            {
                completeFramePreview(process,
                    requestedFrame,
                    generation,
                    exitStatus == QProcess::NormalExit && exitCode == 0);
            });
        connect(process,
            &QProcess::errorOccurred,
            this,
            [this, process, requestedFrame, generation](QProcess::ProcessError)
            {
                completeFramePreview(process, requestedFrame, generation, false);
            });
        QTimer::singleShot(15000,
            process,
            [this, process, requestedFrame, generation]
            {
                if (previewProcess_ == process && previewGeneration_ == generation)
                {
                    process->kill();
                    completeFramePreview(process, requestedFrame, generation, false);
                }
            });
        process->start(request_.ffmpegPath, arguments, QIODevice::ReadOnly);
    }

    void VideoReviewDialog::completeFramePreview(
        QProcess *process, const int frame, const quint64 generation, const bool processSucceeded)
    {
        if (process == nullptr || previewProcess_ != process || previewGeneration_ != generation)
        {
            return;
        }

        previewProcess_ = nullptr;
        disconnect(process, nullptr, this, nullptr);
        QImage image;
        if (processSucceeded)
        {
            image.loadFromData(process->readAllStandardOutput(), "PNG");
        }
        process->deleteLater();

        if (frame != currentFrame_)
        {
            return;
        }
        if (!image.isNull())
        {
            frameCacheOrder_.removeAll(frame);
            while (frameCacheOrder_.size() >= 16)
            {
                frameCache_.remove(frameCacheOrder_.takeFirst());
            }
            frameCache_.insert(frame, image);
            frameCacheOrder_.push_back(frame);
        }
        canvas_->setFrame(frame, std::move(image));
    }

    void VideoReviewDialog::cancelFramePreview()
    {
        ++previewGeneration_;
        if (previewProcess_ == nullptr)
        {
            return;
        }
        QProcess *process = previewProcess_;
        previewProcess_ = nullptr;
        disconnect(process, nullptr, this, nullptr);
        if (process->state() != QProcess::NotRunning)
        {
            process->kill();
        }
        process->deleteLater();
    }

    void VideoReviewDialog::setTrackIncluded(int id, bool included)
    {
        if (included)
        {
            excludedTrackIds_.remove(id);
        }
        else
        {
            excludedTrackIds_.insert(id);
        }
        syncTrackItem(id);
        canvas_->refresh();
        timeline_->update();
        updateSummary();
    }

    void VideoReviewDialog::syncTrackItem(int id)
    {
        const QSignalBlocker blocker(trackList_);
        for (int i = 0; i < trackList_->count(); ++i)
        {
            auto *item = trackList_->item(i);
            if (!item->data(kManualTrackRole).toBool() && item->data(kTrackIdRole).toInt() == id)
            {
                item->setCheckState(excludedTrackIds_.contains(id) ? Qt::Unchecked : Qt::Checked);
                break;
            }
        }
    }

    VideoReviewManualTrack *VideoReviewDialog::manualTrack(const int id)
    {
        const auto it = std::find_if(manualTracks_.begin(),
            manualTracks_.end(),
            [id](const VideoReviewManualTrack &track)
            {
                return track.id == id;
            });
        return it == manualTracks_.end() ? nullptr : &*it;
    }

    const VideoReviewManualTrack *VideoReviewDialog::manualTrack(const int id) const
    {
        const auto it = std::find_if(manualTracks_.cbegin(),
            manualTracks_.cend(),
            [id](const VideoReviewManualTrack &track)
            {
                return track.id == id;
            });
        return it == manualTracks_.cend() ? nullptr : &*it;
    }

    void VideoReviewDialog::startNewManualTrack()
    {
        selectManualTrack(0);
        setDrawingMode(true, 0);
    }

    void VideoReviewDialog::startManualKeyframe()
    {
        if (manualTrack(selectedManualTrackId_) == nullptr)
        {
            return;
        }
        setDrawingMode(true, selectedManualTrackId_);
    }

    void VideoReviewDialog::setDrawingMode(const bool enabled, const int trackId)
    {
        drawingTrackId_ = enabled ? trackId : 0;
        canvas_->setDrawingMode(enabled);
        drawStatusLabel_->setVisible(enabled);
        if (enabled)
        {
            drawStatusLabel_->setText(
                trackId == 0
                    ? tr("Drag a box around the missed region on the current frame.")
                    : tr("Drag the new box for manual track %1 on this frame.").arg(trackId));
        }
        addManualTrackButton_->setEnabled(!enabled);
        addKeyframeButton_->setEnabled(!enabled && selectedManualTrackId_ != 0);
        setStartButton_->setEnabled(!enabled && selectedManualTrackId_ != 0);
        setEndButton_->setEnabled(!enabled && selectedManualTrackId_ != 0);
        removeManualTrackButton_->setEnabled(!enabled && selectedManualTrackId_ != 0);
    }

    void VideoReviewDialog::addOrUpdateManualKeyframe(const int frame, const QRectF &rect)
    {
        const QRectF bounds(QPointF(0.0, 0.0), QSizeF(request_.frameSize));
        const QRectF clipped = rect.normalized().intersected(bounds);
        if (clipped.width() < 2.0 || clipped.height() < 2.0)
        {
            setDrawingMode(false);
            return;
        }

        VideoReviewManualTrack *track = manualTrack(drawingTrackId_);
        if (track == nullptr)
        {
            VideoReviewManualTrack added;
            added.id = nextManualTrackId_++;
            added.startFrame = frame;
            added.endFrame = frame;
            added.keyframes.push_back({frame, clipped, false});
            manualTracks_.push_back(std::move(added));
            track = &manualTracks_.back();

            auto *item = new QListWidgetItem(trackList_);
            item->setData(kManualTrackRole, true);
            item->setData(kTrackIdRole, track->id);
            item->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable);
        }
        else
        {
            const auto position = std::lower_bound(track->keyframes.begin(),
                track->keyframes.end(),
                frame,
                [](const VideoReviewBox &box, const int value)
                {
                    return box.frame < value;
                });
            if (position != track->keyframes.end() && position->frame == frame)
            {
                position->rect = clipped;
            }
            else
            {
                track->keyframes.insert(position, {frame, clipped, false});
            }
            track->startFrame = std::min(track->startFrame, frame);
            track->endFrame = std::max(track->endFrame, frame);
        }

        const int trackId = track->id;
        setDrawingMode(false);
        selectManualTrack(trackId);
        updateManualTrackItem(trackId);
        canvas_->refresh();
        timeline_->update();
        updateSummary();
    }

    void VideoReviewDialog::setManualTrackBoundary(const bool start)
    {
        auto *track = manualTrack(selectedManualTrackId_);
        if (track == nullptr || track->keyframes.isEmpty())
        {
            return;
        }

        auto boundaryRect = manualTrackRectAtFrame(*track, currentFrame_);
        if (!boundaryRect)
        {
            boundaryRect = currentFrame_ < track->startFrame ? track->keyframes.front().rect
                                                             : track->keyframes.back().rect;
        }
        if (start)
        {
            track->startFrame = currentFrame_;
            track->endFrame = std::max(track->endFrame, currentFrame_);
        }
        else
        {
            track->endFrame = currentFrame_;
            track->startFrame = std::min(track->startFrame, currentFrame_);
        }
        track->keyframes.removeIf(
            [track](const VideoReviewBox &box)
            {
                return box.frame < track->startFrame || box.frame > track->endFrame;
            });
        const auto position = std::lower_bound(track->keyframes.begin(),
            track->keyframes.end(),
            currentFrame_,
            [](const VideoReviewBox &box, const int value)
            {
                return box.frame < value;
            });
        if (position == track->keyframes.end() || position->frame != currentFrame_)
        {
            track->keyframes.insert(position, {currentFrame_, *boundaryRect, false});
        }
        updateManualTrackItem(track->id);
        canvas_->refresh();
        timeline_->update();
    }

    void VideoReviewDialog::removeSelectedManualTrack()
    {
        const int id = selectedManualTrackId_;
        if (id == 0)
        {
            return;
        }
        manualTracks_.removeIf(
            [id](const VideoReviewManualTrack &track)
            {
                return track.id == id;
            });
        for (int index = trackList_->count() - 1; index >= 0; --index)
        {
            auto *item = trackList_->item(index);
            if (item->data(kManualTrackRole).toBool() && item->data(kTrackIdRole).toInt() == id)
            {
                delete trackList_->takeItem(index);
                break;
            }
        }
        selectManualTrack(0);
        canvas_->refresh();
        timeline_->update();
        updateSummary();
    }

    void VideoReviewDialog::selectManualTrack(const int id)
    {
        selectedManualTrackId_ = manualTrack(id) != nullptr ? id : 0;
        canvas_->setSelectedManualTrack(selectedManualTrackId_);
        setDrawingMode(false);
        const QSignalBlocker blocker(trackList_);
        if (selectedManualTrackId_ == 0)
        {
            if (trackList_->currentItem() != nullptr
                && trackList_->currentItem()->data(kManualTrackRole).toBool())
            {
                trackList_->setCurrentItem(nullptr);
            }
            return;
        }
        for (int index = 0; index < trackList_->count(); ++index)
        {
            auto *item = trackList_->item(index);
            if (item->data(kManualTrackRole).toBool()
                && item->data(kTrackIdRole).toInt() == selectedManualTrackId_)
            {
                trackList_->setCurrentItem(item);
                break;
            }
        }
    }

    void VideoReviewDialog::updateManualTrackItem(const int id)
    {
        const auto *track = manualTrack(id);
        if (track == nullptr)
        {
            return;
        }
        for (int index = 0; index < trackList_->count(); ++index)
        {
            auto *item = trackList_->item(index);
            if (item->data(kManualTrackRole).toBool() && item->data(kTrackIdRole).toInt() == id)
            {
                item->setText(tr("Manual %1  ·  %2–%3  ·  %n keyframe(s)",
                    nullptr,
                    static_cast<int>(track->keyframes.size()))
                        .arg(id)
                        .arg(formatTime(track->startFrame / request_.fps))
                        .arg(formatTime(track->endFrame / request_.fps)));
                break;
            }
        }
    }

    void VideoReviewDialog::updateSummary()
    {
        const qsizetype includedAutomatic = request_.tracks.size() - excludedTrackIds_.size();
        QString text = tr("%1 of %2 automatic tracks included · %3 manual")
                           .arg(includedAutomatic)
                           .arg(request_.tracks.size())
                           .arg(manualTracks_.size());
        if (!request_.uncoveredSpans.isEmpty())
        {
            text += QStringLiteral(" · ")
                    + tr("%n tracking gap(s) found before review",
                        nullptr,
                        static_cast<int>(request_.uncoveredSpans.size()));
        }
        summaryLabel_->setText(text);
    }
}
