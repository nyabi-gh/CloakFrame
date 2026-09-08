#include "cloakframe/Detector.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/VideoReviewTypes.hpp"

#include <QCoreApplication>
#include <QEventLoop>
#include <QFileInfo>
#include <QProcess>
#include <QTemporaryDir>
#include <QThread>

#include <cassert>

namespace
{
    class GappedDetector final : public cloakframe::Detector
    {
    public:
        cloakframe::DetectionResult detect(const cv::Mat &, float, float) override
        {
            const int frame = frame_++;
            if ((frame >= 20 && frame < 46) || (frame >= 70 && frame < 96))
                return {};
            return {{{cv::Rect2f(40, 30, 80, 60), 0.9F}}, 0};
        }

    private:
        int frame_ = 0;
    };
    class Reviewer final : public QObject
    {
        Q_OBJECT
    public:
        int acknowledge = 0;
        bool exclude = false;
        QVector<cloakframe::UncoveredSpan> gaps;
    public slots:
        cloakframe::VideoReviewResult requestVideoReview(
            const cloakframe::VideoReviewRequest &request)
        {
            assert(request.initialFrame == 20);
            gaps = request.uncoveredSpans;
            cloakframe::VideoReviewResult result;
            for (int i = 0; i < acknowledge && i < gaps.size(); ++i)
                result.acknowledgedGapIndices.push_back(i);
            // Invalid and duplicate indices must not inflate acknowledgement counts.
            result.acknowledgedGapIndices.push_back(-1);
            result.acknowledgedGapIndices.push_back(999999);
            if (acknowledge > 0)
                result.acknowledgedGapIndices.push_back(0);
            if (exclude)
                result.excludedTrackIds.push_back(request.tracks.front().id);
            // A manual mask elsewhere does not acknowledge any interval by itself.
            result.addedTracks.push_back({1, 20, 95, {{20, QRectF(0, 0, 10, 10), false}}});
            return result;
        }
    };
}
int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    qRegisterMetaType<cloakframe::VideoReviewRequest>();
    qRegisterMetaType<cloakframe::VideoReviewResult>();
    const auto tools = cloakframe::locateFfmpegTools();
    if (!tools)
        return 77;
    QTemporaryDir temp;
    assert(temp.isValid());
    const QString source = temp.filePath("input.mp4");
    QProcess generate;
    generate.start(tools->ffmpegPath,
        {"-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=white:size=320x240:rate=30:duration=4",
            "-c:v",
            "libx264",
            source});
    assert(generate.waitForFinished(30000) && generate.exitCode() == 0);
    for (int mode = 0; mode < 4; ++mode)
    {
        Reviewer reviewer;
        reviewer.acknowledge = mode;
        reviewer.exclude = mode == 3;
        cloakframe::ProcessingRequest request;
        request.inputs = {source};
        request.outputDirectory = temp.filePath(QStringLiteral("out%1").arg(mode));
        request.reviewEnabled = true;
        request.reviewReceiver = &reviewer;
        request.initialVideoReviewFrame = 20;
        const QString output = request.outputDirectory + "/input.mp4";
        cloakframe::DetectorCache cache;
        cache.face = std::make_shared<GappedDetector>();
        cache.videoFace = std::make_shared<GappedDetector>();
        QThread thread;
        auto *worker = new cloakframe::ProcessorWorker(request, std::move(cache));
        worker->moveToThread(&thread);
        cloakframe::RunSummary summary;
        cloakframe::FileResult file;
        cloakframe::RunOutcome outcome = cloakframe::RunOutcome::Failed;
        QEventLoop loop;
        QObject::connect(&thread, &QThread::started, worker, &cloakframe::ProcessorWorker::process);
        QObject::connect(worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            &app,
            [&](cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(worker,
            &cloakframe::ProcessorWorker::fileResultAvailable,
            &app,
            [&](cloakframe::FileResult value)
            {
                file = std::move(value);
            });
        QObject::connect(worker,
            &cloakframe::ProcessorWorker::finished,
            &loop,
            [&](cloakframe::RunOutcome value)
            {
                outcome = value;
                loop.quit();
            });
        QObject::connect(&thread, &QThread::finished, worker, &QObject::deleteLater);
        thread.start();
        loop.exec();
        thread.quit();
        assert(thread.wait());
        assert(reviewer.gaps.size() == 2);
        assert(summary.trackingGapFrames > 0);
        qint64 expectedPending = 0;
        for (int i = mode; i < reviewer.gaps.size(); ++i)
            expectedPending += reviewer.gaps[i].frameCount();
        assert(summary.pendingTrackingGapFrames == expectedPending);
        assert(summary.excludedTracks == (mode == 3 ? 1 : 0));
        assert(outcome
               == (mode == 2 ? cloakframe::RunOutcome::Completed
                             : cloakframe::RunOutcome::CompletedWithWarnings));
        assert(file.status
               == (mode == 2 ? cloakframe::FileResultStatus::Saved
                             : cloakframe::FileResultStatus::NeedsReview));
        assert(QFileInfo::exists(output));
        assert(QFileInfo(file.outputPath).canonicalFilePath()
               == QFileInfo(output).canonicalFilePath());
        int acknowledged = 0;
        for (const auto &issue : file.issues)
            if (issue.kind == cloakframe::FileIssueKind::TrackingGap)
                acknowledged += issue.acknowledged;
        assert(acknowledged == std::min(mode, 2));
    }
    return 0;
}
#include "test_worker_video_review.moc"
