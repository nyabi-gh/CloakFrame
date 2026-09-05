#pragma once

#include "cloakframe/VideoReviewTypes.hpp"

#include <QDialog>
#include <QHash>
#include <QImage>
#include <QList>
#include <QSet>

class QLabel;
class QListWidget;
class QProcess;
class QPushButton;
class QTimer;

namespace cloakframe
{
    class VideoReviewCanvas;
    class VideoTimeline;

    class VideoReviewDialog final : public QDialog
    {
        Q_OBJECT

    public:
        explicit VideoReviewDialog(VideoReviewRequest request, QWidget *parent = nullptr);

        ~VideoReviewDialog() override;

        [[nodiscard]] VideoReviewResult reviewResult() const;

        void reject() override;

    private:
        void setFrame(int frame);
        void loadFramePreview();
        void completeFramePreview(
            QProcess *process, int frame, quint64 generation, bool processSucceeded);
        void cancelFramePreview();
        void setTrackIncluded(int id, bool included);
        void syncTrackItem(int id);
        void startNewManualTrack();
        void startManualKeyframe();
        void addOrUpdateManualKeyframe(int frame, const QRectF &rect);
        void setManualTrackBoundary(bool start);
        void removeSelectedManualTrack();
        void selectManualTrack(int id);
        void setDrawingMode(bool enabled, int trackId = 0);
        void updateManualTrackItem(int id);
        void updateSummary();
        void navigateGap(bool next);
        void updateNavigation();

        [[nodiscard]] VideoReviewManualTrack *manualTrack(int id);
        [[nodiscard]] const VideoReviewManualTrack *manualTrack(int id) const;

        VideoReviewRequest request_;
        QSet<int> excludedTrackIds_;
        QVector<VideoReviewManualTrack> manualTracks_;
        QHash<int, QImage> frameCache_;
        QList<int> frameCacheOrder_;
        VideoReviewCanvas *canvas_ = nullptr;
        VideoTimeline *timeline_ = nullptr;
        QListWidget *trackList_ = nullptr;
        QListWidget *gapList_ = nullptr;
        QPushButton *previousGapButton_ = nullptr;
        QPushButton *nextGapButton_ = nullptr;
        QPushButton *previousFrameButton_ = nullptr;
        QPushButton *nextFrameButton_ = nullptr;
        QLabel *timeLabel_ = nullptr;
        QLabel *summaryLabel_ = nullptr;
        QLabel *drawStatusLabel_ = nullptr;
        QTimer *seekTimer_ = nullptr;
        QProcess *previewProcess_ = nullptr;
        QPushButton *addManualTrackButton_ = nullptr;
        QPushButton *addKeyframeButton_ = nullptr;
        QPushButton *setStartButton_ = nullptr;
        QPushButton *setEndButton_ = nullptr;
        QPushButton *removeManualTrackButton_ = nullptr;
        int currentFrame_ = 0;
        int selectedManualTrackId_ = 0;
        int drawingTrackId_ = 0;
        int nextManualTrackId_ = 1;
        quint64 previewGeneration_ = 0;
        VideoReviewDecision decision_ = VideoReviewDecision::Encode;
    };
}
