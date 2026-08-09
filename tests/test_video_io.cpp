#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/VideoReviewTypes.hpp"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QProcess>
#include <QTemporaryDir>

#include <opencv2/core.hpp>
#include <opencv2/imgcodecs.hpp>

#include <algorithm>
#include <atomic>
#include <cassert>
#include <cmath>
#include <cstdio>
#include <vector>

namespace
{
    constexpr int kSkipExitCode = 77;

    bool generateSample(
        const cloakframe::FfmpegTools &tools, const QString &path, int durationSeconds = 2)
    {
        const QString duration = QString::number(durationSeconds);
        QProcess process;
        process.start(tools.ffmpegPath,
            {"-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=30:duration=" + duration,
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=" + duration,
                "-metadata",
                "title=CloakFrameTestTitle",
                "-metadata",
                "location=+37.5665+126.9780/",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                path});
        if (!process.waitForStarted(15000) || !process.waitForFinished(60000))
        {
            process.kill();
            return false;
        }
        return process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0;
    }

    bool runFfmpeg(const cloakframe::FfmpegTools &tools, const QStringList &arguments)
    {
        QProcess process;
        process.start(tools.ffmpegPath, arguments);
        if (!process.waitForStarted(15000) || !process.waitForFinished(60000))
        {
            process.kill();
            return false;
        }
        return process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0;
    }

    // A video whose stream starts after the container origin, with audio that starts at it.
    bool generateOffsetStartSample(
        const cloakframe::FfmpegTools &tools, const QString &directory, const QString &path)
    {
        const QString video = directory + "/offset-video.mp4";
        const QString audio = directory + "/offset-audio.m4a";
        return runFfmpeg(tools,
                   {"-v",
                       "error",
                       "-y",
                       "-f",
                       "lavfi",
                       "-i",
                       "testsrc=size=160x120:rate=10:duration=4",
                       "-c:v",
                       "libx264",
                       "-pix_fmt",
                       "yuv420p",
                       video})
               && runFfmpeg(tools,
                   {"-v",
                       "error",
                       "-y",
                       "-f",
                       "lavfi",
                       "-i",
                       "sine=frequency=440:duration=6",
                       "-c:a",
                       "aac",
                       audio})
               && runFfmpeg(tools,
                   {"-v",
                       "error",
                       "-y",
                       "-itsoffset",
                       "2",
                       "-i",
                       video,
                       "-i",
                       audio,
                       "-map",
                       "0:v:0",
                       "-map",
                       "1:a:0",
                       "-c",
                       "copy",
                       path});
    }

    // A container that still probes as a supported video, but whose bitstream makes the decoder
    // report errors and carry on. Which byte a given corruption lands on depends on the local
    // encoder, so candidates are tried until one reproduces the hazard: a decode that reports
    // errors and still exits 0.
    bool generateRecoverablyCorruptSample(
        const cloakframe::FfmpegTools &tools, const QString &directory, const QString &path)
    {
        const QString clean = directory + "/decode-clean.mp4";
        if (!runFfmpeg(tools,
                {"-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=128x96:rate=10:duration=6",
                    "-c:v",
                    "libx264",
                    "-g",
                    "10",
                    "-pix_fmt",
                    "yuv420p",
                    clean}))
        {
            return false;
        }

        for (const int amount : {1000, 100, 10})
        {
            if (!runFfmpeg(tools,
                    {"-v",
                        "error",
                        "-y",
                        "-i",
                        clean,
                        "-map",
                        "0:v:0",
                        "-c",
                        "copy",
                        "-bsf:v",
                        QString("noise=amount=%1").arg(amount),
                        path}))
            {
                continue;
            }

            QProcess decode;
            decode.setStandardOutputFile(QProcess::nullDevice());
            decode.start(tools.ffmpegPath,
                {"-v",
                    "error",
                    "-nostdin",
                    "-y",
                    "-i",
                    path,
                    "-map",
                    "0:v:0",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "bgr24",
                    "-"});
            if (!decode.waitForStarted(15000) || !decode.waitForFinished(60000))
            {
                decode.kill();
                continue;
            }
            const bool reportedErrors = !decode.readAllStandardError().trimmed().isEmpty();
            if (decode.exitStatus() == QProcess::NormalExit && decode.exitCode() == 0
                && reportedErrors)
            {
                return true;
            }
        }
        return false;
    }

    QString rawProbeOutput(const cloakframe::FfmpegTools &tools, const QString &path)
    {
        QProcess process;
        process.start(tools.ffprobePath,
            {"-v", "error", "-print_format", "json", "-show_streams", "-show_format", path});
        process.waitForStarted(15000);
        process.waitForFinished(60000);
        return QString::fromUtf8(process.readAllStandardOutput());
    }

    void testUnsupportedReasons()
    {
        cloakframe::VideoInfo info;
        info.width = 1920;
        info.height = 1080;
        info.fpsNum = 30;
        info.fpsDen = 1;
        info.videoCodec = "h264";
        info.pixelFormat = "yuv420p";
        assert(cloakframe::videoUnsupportedReason(info).isEmpty());

        info.width = 3840;
        info.height = 2160;
        info.fpsNum = 60;
        assert(cloakframe::videoUnsupportedReason(info).isEmpty());
        info.width = 1920;
        info.height = 1080;
        info.fpsNum = 30;

        info.videoCodec = "hevc";
        assert(cloakframe::videoUnsupportedReason(info).isEmpty());

        info.videoCodec = "vp8";
        assert(cloakframe::videoUnsupportedReason(info).isEmpty());

        info.videoCodec = "vp9";
        assert(cloakframe::videoUnsupportedReason(info).isEmpty());

        info.videoCodec = "av1";
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.videoCodec = "h264";

        info.pixelFormat = "yuv420p10le";
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.pixelFormat = "yuv420p";

        info.colorTransfer = "smpte2084";
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.colorTransfer.clear();

        info.fpsNum = 0;
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());

        info.fpsNum = 30;
        info.width = cloakframe::kMaxVideoDimension + 1;
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.width = 1920;

        info.fpsNum = 241;
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.fpsNum = 30;

        info.durationSeconds = cloakframe::kMaxVideoDurationSeconds + 1.0;
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
        info.durationSeconds = 0.0;

        info.estimatedFrameCount = cloakframe::kMaxVideoFrameCount + 1;
        assert(!cloakframe::videoUnsupportedReason(info).isEmpty());
    }

    void testSupportedExtensions()
    {
        assert(cloakframe::isSupportedVideo("clip.mp4"));
        assert(cloakframe::isSupportedVideo("CLIP.MOV"));
        assert(cloakframe::isSupportedVideo("clip.m4v"));
        assert(cloakframe::isSupportedVideo("clip.webm"));
        assert(cloakframe::isSupportedVideo("CLIP.WEBM"));
        assert(!cloakframe::isSupportedVideo("clip.mkv"));
        assert(!cloakframe::isSupportedVideo("clip.jpg"));
    }

    void testCrfPresets()
    {
        assert(cloakframe::crfForQuality(cloakframe::VideoQuality::HighQuality) == 18);
        assert(cloakframe::crfForQuality(cloakframe::VideoQuality::Balanced) == 21);
        assert(cloakframe::crfForQuality(cloakframe::VideoQuality::SpaceSaver) == 24);
    }

    void testVideoStrongThresholdNeverExceedsUserThreshold()
    {
        const auto close = [](float actual, float expected)
        {
            return std::abs(actual - expected) < 0.0001F;
        };
        assert(close(cloakframe::videoStrongScoreThreshold(0.05F), 0.05F));
        assert(close(cloakframe::videoStrongScoreThreshold(0.20F), 0.20F));
        assert(close(cloakframe::videoStrongScoreThreshold(0.50F), 0.40F));
        assert(close(cloakframe::videoStrongScoreThreshold(0.90F), 0.80F));
    }

    void testVideoMaskingPlanIsBoundedByWorkersAndBytes()
    {
        constexpr qint64 constrainedBudget = 256LL * 1024 * 1024;
        constexpr qint64 performanceBudget = 1024LL * 1024 * 1024;
        constexpr qint64 headroom = 48LL * 1024 * 1024;

        const auto fullHd = cloakframe::videoMaskingPlan(1920, 1080, 64, constrainedBudget);
        assert(fullHd.workerCount <= 8);
        assert(fullHd.batchFrames <= 16);
        assert(fullHd.batchFrames * fullHd.frameBytes + headroom <= constrainedBudget);

        const auto constrainedFourK =
            cloakframe::videoMaskingPlan(3840, 2160, 64, constrainedBudget);
        assert(constrainedFourK.workerCount == 3);
        assert(constrainedFourK.batchFrames == 3);
        assert(constrainedFourK.batchFrames * constrainedFourK.frameBytes + headroom
               <= constrainedBudget);

        const auto performanceFourK =
            cloakframe::videoMaskingPlan(3840, 2160, 64, performanceBudget);
        assert(performanceFourK.workerCount == 8);
        assert(performanceFourK.batchFrames == 15);
        assert(performanceFourK.batchFrames * performanceFourK.frameBytes + headroom
               <= performanceBudget);

        const auto eightK = cloakframe::videoMaskingPlan(7680, 4320, 64, performanceBudget);
        assert(eightK.workerCount == 3);
        assert(eightK.batchFrames == 3);
        assert(eightK.batchFrames * eightK.frameBytes + headroom <= performanceBudget);

        const auto unknownCpuCount = cloakframe::videoMaskingPlan(320, 240, 0, constrainedBudget);
        assert(unknownCpuCount.workerCount == 1);
        assert(unknownCpuCount.batchFrames == 2);
    }

    // CF-006: the review preview and the pass that actually masks pixels must agree on what
    // "frame N" is, or a box placed in review lands on a different frame in the output.
    void testPreviewAndReaderAgreeOnFrameIndex(
        const cloakframe::FfmpegTools &tools, const QString &directory)
    {
        const QString source = directory + "/offset-start.mp4";
        assert(generateOffsetStartSample(tools, directory, source));

        const auto info = cloakframe::probeVideo(tools, source);
        assert(info);
        assert(std::abs(info->startTimeSeconds - 2.0) < 0.001);

        cloakframe::VideoFrameReader reader;
        assert(reader.open(tools, source, *info));
        std::vector<QByteArray> readerFrames;
        cv::Mat frame;
        while (reader.readFrame(frame))
        {
            assert(frame.isContinuous());
            readerFrames.push_back(QCryptographicHash::hash(
                QByteArrayView(
                    frame.data, static_cast<qsizetype>(frame.total() * frame.elemSize())),
                QCryptographicHash::Md5));
        }
        // The stream holds 40 frames; the container origin two seconds earlier must not pad it.
        assert(readerFrames.size() == 40);
        assert(readerFrames.front() != readerFrames[10]);

        cloakframe::VideoReviewRequest request;
        request.sourcePath = source;
        request.ffmpegPath = tools.ffmpegPath;
        request.frameSize = QSize(info->displayWidth(), info->displayHeight());
        request.fps = info->fps();
        request.startTimeSeconds = info->startTimeSeconds;
        request.fpsNum = info->fpsNum;
        request.fpsDen = info->fpsDen;
        request.frameCount = static_cast<int>(readerFrames.size());

        // Cross the seek margin so both the direct and the seeking branch are exercised.
        for (const int index : {0, 1, 17, 30, 31, 39})
        {
            const auto arguments = cloakframe::videoPreviewFrameArguments(request, index);
            assert(!arguments.isEmpty());
            QProcess process;
            process.start(tools.ffmpegPath, arguments);
            assert(process.waitForStarted(15000) && process.waitForFinished(60000));
            const QByteArray png = process.readAllStandardOutput();
            assert(!png.isEmpty());

            const cv::Mat decoded = cv::imdecode(
                cv::Mat(1, static_cast<int>(png.size()), CV_8UC1, const_cast<char *>(png.data())),
                cv::IMREAD_COLOR);
            assert(!decoded.empty());
            const auto previewHash = QCryptographicHash::hash(
                QByteArrayView(
                    decoded.data, static_cast<qsizetype>(decoded.total() * decoded.elemSize())),
                QCryptographicHash::Md5);
            assert(previewHash == readerFrames[static_cast<std::size_t>(index)]);
        }
    }

    // CF-009: anamorphic sources must not be published as square-pixel.
    void testAnamorphicOutputKeepsDisplayAspect(
        const cloakframe::FfmpegTools &tools, const QString &directory)
    {
        const QString source = directory + "/anamorphic.mp4";
        assert(runFfmpeg(tools,
            {"-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=720x576:rate=25:duration=1",
                "-vf",
                "setsar=16/15",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                source}));

        const auto info = cloakframe::probeVideo(tools, source);
        assert(info);
        assert(info->sarNum == 16 && info->sarDen == 15);

        const QString output = directory + "/anamorphic-out.mp4";
        cloakframe::VideoFrameWriter writer;
        assert(writer.open(tools, output, source, *info, 23, false, cloakframe::VideoCodec::H264));
        cloakframe::VideoFrameReader reader;
        assert(reader.open(tools, source, *info));
        cv::Mat frame;
        while (reader.readFrame(frame))
        {
            assert(writer.writeFrame(frame));
        }
        assert(writer.finish());

        const auto written = cloakframe::probeVideo(tools, output);
        assert(written);
        assert(written->width == 720 && written->height == 576);
        assert(written->sarNum == 16 && written->sarDen == 15);
    }

    void testEveryAudioStreamSurvives(
        const cloakframe::FfmpegTools &tools, const QString &directory)
    {
        // Two audio streams, one of them in a codec MP4 cannot carry, so both mapping and
        // per-stream codec selection are exercised.
        const QString source = directory + "/two-audio.mp4";
        assert(runFfmpeg(tools,
            {"-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=128x96:rate=10:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=2",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-map",
                "2:a:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a:0",
                "aac",
                "-c:a:1",
                "flac",
                "-metadata:s:a:1",
                "language=jpn",
                source}));

        const auto info = cloakframe::probeVideo(tools, source);
        assert(info);
        assert(info->audioStreams.size() == 2);
        assert(info->audioStreams.at(0).codec == "aac");
        assert(info->audioStreams.at(1).codec == "flac");
        assert(info->audioStreams.at(1).language == "jpn");

        const QString output = directory + "/two-audio-out.mp4";
        cloakframe::VideoProcessOptions options;
        options.hardwareEncoder = false;
        std::atomic<bool> cancelled{false};
        const auto result = cloakframe::processVideo(
            tools,
            source,
            output,
            *info,
            options,
            [](const cv::Mat &)
            {
                return cloakframe::FaceDetections{};
            },
            cancelled);
        assert(result.status == cloakframe::VideoProcessStatus::Completed);

        const auto written = cloakframe::probeVideo(tools, output);
        assert(written);
        assert(written->audioStreams.size() == 2);
        // The compatible stream is copied; the one MP4 cannot carry is re-encoded.
        assert(written->audioStreams.at(0).codec == "aac");
        assert(written->audioStreams.at(1).codec == "aac");
        assert(written->audioStreams.at(1).language == "jpn");
    }

    void testRecoveredDecodeErrorsDoNotPublish(
        const cloakframe::FfmpegTools &tools, const QString &directory)
    {
        const QString source = directory + "/decode-errors.mp4";
        assert(generateRecoverablyCorruptSample(tools, directory, source));

        // The container itself still describes a supported video, which is what made the
        // damaged stream reach the encoder unnoticed.
        const auto info = cloakframe::probeVideo(tools, source);
        assert(info);
        assert(cloakframe::videoUnsupportedReason(*info).isEmpty());

        const QString output = directory + "/decode-errors-out.mp4";
        cloakframe::VideoProcessOptions options;
        options.hardwareEncoder = false;
        std::atomic<bool> cancelled{false};
        const auto result = cloakframe::processVideo(
            tools,
            source,
            output,
            *info,
            options,
            [](const cv::Mat &)
            {
                return cloakframe::FaceDetections{};
            },
            cancelled);

        assert(result.status == cloakframe::VideoProcessStatus::Failed);
        assert(!result.error.isEmpty());
        assert(!QFile::exists(output));
    }

#ifndef _WIN32
    // Stands in for FFmpeg: it reports every hardware encoder as available, then fails once
    // real frames are piped to one, and records the encoder each encode ran with. Anything
    // else is handed to the real FFmpeg unchanged.
    bool writeHardwareFailingFfmpeg(
        const cloakframe::FfmpegTools &tools, const QString &scriptPath, const QString &logPath)
    {
        QFile script(scriptPath);
        if (!script.open(QIODevice::WriteOnly | QIODevice::Truncate))
        {
            return false;
        }
        const QString body = QStringLiteral(R"(#!/bin/sh
hardware=0
raw=0
previous=""
encoder=""
for argument in "$@"; do
  case "$previous" in -c:v) encoder="$argument" ;; esac
  case "$argument" in
    *_nvenc|*_qsv|*_videotoolbox) hardware=1 ;;
    rawvideo) raw=1 ;;
  esac
  previous="$argument"
done
if [ "$raw" -eq 1 ] && [ -n "$encoder" ]; then
  echo "$encoder" >> "%1"
fi
if [ "$hardware" -eq 1 ]; then
  if [ "$raw" -eq 1 ]; then
    exit 42
  fi
  exit 0
fi
exec "%2" "$@"
)")
                                 .arg(logPath, tools.ffmpegPath);
        script.write(body.toUtf8());
        script.close();
        return script.setPermissions(
            QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);
    }

    void testHardwareEncodeFailureRetriesInSoftware(
        const cloakframe::FfmpegTools &tools, const QString &directory)
    {
        const QString source = directory + "/hardware-retry.mp4";
        assert(runFfmpeg(tools,
            {"-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=96x72:rate=10:duration=1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                source}));

        const QString scriptPath = directory + "/ffmpeg-hardware-fails.sh";
        const QString logPath = directory + "/encoder-attempts.txt";
        assert(writeHardwareFailingFfmpeg(tools, scriptPath, logPath));

        const auto info = cloakframe::probeVideo(tools, source);
        assert(info);

        cloakframe::FfmpegTools failing = tools;
        failing.ffmpegPath = scriptPath;

        const QString output = directory + "/hardware-retry-out.mp4";
        cloakframe::VideoProcessOptions options;
        options.hardwareEncoder = true;
        std::atomic<bool> cancelled{false};
        const auto result = cloakframe::processVideo(
            failing,
            source,
            output,
            *info,
            options,
            [](const cv::Mat &)
            {
                return cloakframe::FaceDetections{};
            },
            cancelled);

        assert(result.status == cloakframe::VideoProcessStatus::Completed);
        assert(result.encoderName == "libx264");

        QFile attempts(logPath);
        assert(attempts.open(QIODevice::ReadOnly));
        const QStringList encoders =
            QString::fromUtf8(attempts.readAll()).split('\n', Qt::SkipEmptyParts);
        // The hardware encoder has to have been tried first, or the software result would
        // prove nothing about the fallback.
        assert(std::any_of(encoders.cbegin(),
            encoders.cend(),
            [](const QString &encoder)
            {
                return encoder != "libx264";
            }));
        assert(encoders.contains("libx264"));

        const auto written = cloakframe::probeVideo(tools, output);
        assert(written);
        assert(written->width == 96 && written->height == 72);
    }
#endif
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);

    testUnsupportedReasons();
    std::puts("unsupported reasons: ok");
    testSupportedExtensions();
    std::puts("supported extensions: ok");
    testCrfPresets();
    std::puts("crf presets: ok");
    testVideoStrongThresholdNeverExceedsUserThreshold();
    std::puts("video strong-score floor: ok");
    testVideoMaskingPlanIsBoundedByWorkersAndBytes();
    std::puts("bounded video masking plan: ok");

    QString locateError;
    const auto tools = cloakframe::locateFfmpegTools(&locateError);
    if (!tools)
    {
        std::printf("SKIP: %s\n", locateError.toUtf8().constData());
        return kSkipExitCode;
    }
    std::printf("using %s\n", tools->versionLine.toUtf8().constData());

    QTemporaryDir tempDir;
    assert(tempDir.isValid());

    testPreviewAndReaderAgreeOnFrameIndex(*tools, tempDir.path());
    std::puts("preview and reader frame identity: ok");
    testAnamorphicOutputKeepsDisplayAspect(*tools, tempDir.path());
    std::puts("anamorphic display aspect preserved: ok");
    testRecoveredDecodeErrorsDoNotPublish(*tools, tempDir.path());
    std::puts("recovered decode errors do not publish: ok");
    testEveryAudioStreamSurvives(*tools, tempDir.path());
    std::puts("every audio stream survives: ok");
#ifndef _WIN32
    testHardwareEncodeFailureRetriesInSoftware(*tools, tempDir.path());
    std::puts("hardware encode failure retries in software: ok");
#endif

    const QString samplePath = tempDir.filePath("sample.mp4");
    const QString outputPath = tempDir.filePath("out/redacted.mp4");
    QDir().mkpath(tempDir.filePath("out"));
    const auto stagingLeftovers = [](const QString &dirPath)
    {
        return QDir(dirPath)
            .entryList({QStringLiteral(".cloakframe-*")},
                QDir::AllEntries | QDir::Hidden | QDir::NoDotAndDotDot)
            .size();
    };

    assert(generateSample(*tools, samplePath));
    std::puts("sample generated: ok");

    QString probeError;
    const auto info = cloakframe::probeVideo(*tools, samplePath, &probeError);
    assert(info.has_value());
    assert(info->width == 320);
    assert(info->height == 240);
    assert(info->fpsNum == 30 && info->fpsDen == 1);
    assert(info->videoCodec == "h264");
    assert(info->audioStreams.size() == 1);
    assert(info->audioStreams.at(0).codec == "aac");
    assert(!info->isVfr);
    assert(cloakframe::videoUnsupportedReason(*info).isEmpty());
    assert(info->estimatedFrameCount >= 55 && info->estimatedFrameCount <= 65);
    std::puts("probe: ok");

    cloakframe::VideoFrameReader reader;
    assert(reader.open(*tools, samplePath, *info));

    cloakframe::VideoFrameWriter writer;
    assert(writer.open(*tools,
        outputPath,
        samplePath,
        *info,
        cloakframe::crfForQuality(cloakframe::VideoQuality::HighQuality)));

    int frameCount = 0;
    cv::Mat frame;
    while (reader.readFrame(frame))
    {
        cv::bitwise_not(frame, frame);
        assert(writer.writeFrame(frame));
        ++frameCount;
    }
    assert(reader.errorString().isEmpty());
    assert(frameCount >= 55 && frameCount <= 65);
    assert(writer.finish());
    assert(stagingLeftovers(tempDir.filePath("out")) == 0);
    std::printf("round trip (%d frames): ok\n", frameCount);

    {
        const QString guardedPath = tempDir.filePath("out/guarded.mp4");
        cloakframe::VideoFrameWriter guardedWriter;
        assert(guardedWriter.open(*tools,
            guardedPath,
            samplePath,
            *info,
            cloakframe::crfForQuality(cloakframe::VideoQuality::SpaceSaver),
            false));
        cloakframe::VideoFrameReader guardedReader;
        assert(guardedReader.open(*tools, samplePath, *info));
        cv::Mat guardedFrame;
        assert(guardedReader.readFrame(guardedFrame));
        assert(guardedWriter.writeFrame(guardedFrame));
        assert(!guardedWriter.finish(
            []
            {
                return false;
            }));
        assert(!QFile::exists(guardedPath));
        assert(stagingLeftovers(tempDir.filePath("out")) == 0);
        std::puts("video publish guard: ok");
    }

    const auto outInfo = cloakframe::probeVideo(*tools, outputPath, &probeError);
    assert(outInfo.has_value());
    assert(outInfo->width == 320);
    assert(outInfo->height == 240);
    assert(outInfo->videoCodec == "h264");
    assert(outInfo->audioStreams.size() == 1);
    assert(outInfo->audioStreams.at(0).codec == "aac");
    assert(outInfo->durationSeconds > 1.5 && outInfo->durationSeconds < 2.5);

    const QString rawOutput = rawProbeOutput(*tools, outputPath);
    assert(!rawOutput.contains("CloakFrameTestTitle"));
    assert(!rawOutput.contains("37.5665"));
    std::puts("output verification: ok");

    {
        cloakframe::VideoInfo fourKInfo = *info;
        fourKInfo.width = 3840;
        fourKInfo.height = 2160;
        fourKInfo.rotation = 0;
        fourKInfo.fpsNum = 60;
        fourKInfo.fpsDen = 1;
        fourKInfo.durationSeconds = 0.05;
        fourKInfo.estimatedFrameCount = 3;

        const QString fourKPath = tempDir.filePath("out/4k60-source.mp4");
        cloakframe::VideoFrameWriter fourKWriter;
        assert(fourKWriter.open(*tools,
            fourKPath,
            samplePath,
            fourKInfo,
            cloakframe::crfForQuality(cloakframe::VideoQuality::SpaceSaver),
            false));
        cv::Mat fourKFrame(2160, 3840, CV_8UC3);
        for (int frameIndex = 0; frameIndex < 3; ++frameIndex)
        {
            fourKFrame.setTo(
                cv::Scalar(20 + frameIndex * 30, 40 + frameIndex * 20, 60 + frameIndex * 10));
            assert(fourKWriter.writeFrame(fourKFrame));
        }
        assert(fourKWriter.finish());

        const auto fourKProbe = cloakframe::probeVideo(*tools, fourKPath, &probeError);
        assert(fourKProbe.has_value());
        assert(fourKProbe->displayWidth() == 3840);
        assert(fourKProbe->displayHeight() == 2160);
        assert(fourKProbe->fpsNum == 60 && fourKProbe->fpsDen == 1);
        assert(cloakframe::videoUnsupportedReason(*fourKProbe).isEmpty());

        const QString fourKOutput = tempDir.filePath("out/4k60-processed.mp4");
        cloakframe::VideoProcessOptions fourKOptions;
        fourKOptions.hardwareEncoder = false;
        std::atomic<bool> fourKCancelled{false};
        const auto fourKResult = cloakframe::processVideo(
            *tools, fourKPath, fourKOutput, *fourKProbe, fourKOptions, {}, fourKCancelled);
        assert(fourKResult.status == cloakframe::VideoProcessStatus::Completed);
        assert(fourKResult.frameCount == 3);
        const auto processedFourK = cloakframe::probeVideo(*tools, fourKOutput, &probeError);
        assert(processedFourK.has_value());
        assert(processedFourK->displayWidth() == 3840);
        assert(processedFourK->displayHeight() == 2160);
        assert(processedFourK->fpsNum == 60 && processedFourK->fpsDen == 1);
        std::puts("4K60 end-to-end: ok");
    }

    {
        QProcess encoders;
        encoders.start(tools->ffmpegPath, {"-v", "error", "-encoders"});
        encoders.waitForStarted(15000);
        encoders.waitForFinished(60000);
        const QString encoderList = QString::fromUtf8(encoders.readAllStandardOutput());
        if (encoderList.contains("libx265"))
        {
            const QString hevcPath = tempDir.filePath("out/hevc.mp4");
            cloakframe::VideoFrameReader hevcReader;
            assert(hevcReader.open(*tools, samplePath, *info));
            cloakframe::VideoFrameWriter hevcWriter;
            assert(hevcWriter.open(*tools,
                hevcPath,
                samplePath,
                *info,
                cloakframe::crfForQuality(cloakframe::VideoQuality::Balanced),
                false,
                cloakframe::VideoCodec::Hevc));
            assert(hevcWriter.encoderName() == "libx265");
            cv::Mat hevcFrame;
            while (hevcReader.readFrame(hevcFrame))
            {
                assert(hevcWriter.writeFrame(hevcFrame));
            }
            assert(hevcWriter.finish());

            const auto hevcInfo = cloakframe::probeVideo(*tools, hevcPath, &probeError);
            assert(hevcInfo.has_value());
            assert(hevcInfo->videoCodec == "hevc");
            assert(!hevcInfo->audioStreams.empty());
            assert(rawProbeOutput(*tools, hevcPath).contains("hvc1"));
            std::puts("hevc round trip: ok");
        }
        else
        {
            std::puts("hevc round trip: skipped (libx265 unavailable)");
        }
    }

    {
        const QString existingPath = tempDir.filePath("out/existing.mp4");
        QFile existing(existingPath);
        assert(existing.open(QIODevice::WriteOnly));
        assert(existing.write("keep-existing-output") == 20);
        existing.close();

        std::atomic<bool> cancelled{false};
        const auto result =
            cloakframe::processVideo(*tools, samplePath, existingPath, *info, {}, {}, cancelled);
        assert(result.status == cloakframe::VideoProcessStatus::Failed);
        assert(result.error.contains("already exists"));
        assert(existing.open(QIODevice::ReadOnly));
        assert(existing.readAll() == "keep-existing-output");
        std::puts("existing output preservation: ok");
    }

    const QString bogusPath = tempDir.filePath("bogus.mp4");
    {
        QFile bogus(bogusPath);
        assert(bogus.open(QIODevice::WriteOnly));
        assert(bogus.write("this is not a video file") > 0);
    }
    QString corruptError;
    const auto corruptInfo = cloakframe::probeVideo(*tools, bogusPath, &corruptError);
    assert(!corruptInfo.has_value());
    assert(!corruptError.isEmpty());
    std::puts("corrupt input rejection: ok");

    const QString rotatedPath = tempDir.filePath("rotated.mp4");
    QProcess remux;
    remux.start(tools->ffmpegPath,
        {"-v",
            "error",
            "-y",
            "-display_rotation",
            "90",
            "-i",
            samplePath,
            "-c",
            "copy",
            rotatedPath});
    remux.waitForStarted(15000);
    remux.waitForFinished(60000);
    if (remux.exitStatus() == QProcess::NormalExit && remux.exitCode() == 0)
    {
        const auto rotatedInfo = cloakframe::probeVideo(*tools, rotatedPath, &probeError);
        assert(rotatedInfo.has_value());
        assert(rotatedInfo->rotation == 90 || rotatedInfo->rotation == 270);
        assert(rotatedInfo->displayWidth() == 240);
        assert(rotatedInfo->displayHeight() == 320);

        cloakframe::VideoFrameReader rotatedReader;
        assert(rotatedReader.open(*tools, rotatedPath, *rotatedInfo));
        cv::Mat rotatedFrame;
        assert(rotatedReader.readFrame(rotatedFrame));
        assert(rotatedFrame.cols == 240);
        assert(rotatedFrame.rows == 320);
        rotatedReader.close();

        cloakframe::VideoFrameWriter rotatedWriter;
        assert(rotatedWriter.open(*tools,
            tempDir.filePath("rotated-out.mp4"),
            rotatedPath,
            *rotatedInfo,
            cloakframe::crfForQuality(cloakframe::VideoQuality::SpaceSaver)));
        assert(rotatedWriter.writeFrame(rotatedFrame));
        assert(rotatedWriter.finish());

        const auto rotatedOut =
            cloakframe::probeVideo(*tools, tempDir.filePath("rotated-out.mp4"), &probeError);
        assert(rotatedOut.has_value());
        assert(rotatedOut->rotation == 0);
        assert(rotatedOut->width == 240);
        assert(rotatedOut->height == 320);
        std::puts("rotation handling: ok");
    }
    else
    {
        std::puts("rotation handling: skipped (-display_rotation unsupported)");
    }

    {
        cloakframe::VideoFrameReader smallReader;
        assert(smallReader.open(*tools, samplePath, *info, 160));
        assert(smallReader.frameWidth() == 160);
        assert(smallReader.frameHeight() == 120);
        cv::Mat smallFrame;
        assert(smallReader.readFrame(smallFrame));
        assert(smallFrame.cols == 160 && smallFrame.rows == 120);
        std::puts("downscaled decode: ok");
    }

    {
        const QString processedPath = tempDir.filePath("processed.mp4");
        std::atomic<bool> cancelled{false};
        qint64 lastPass2Frame = 0;
        const auto result = cloakframe::processVideo(*tools,
            samplePath,
            processedPath,
            *info,
            {},
            {},
            cancelled,
            [&](int pass, qint64 frameIndex, qint64)
            {
                if (pass == 2)
                {
                    lastPass2Frame = frameIndex;
                }
            });
        assert(result.status == cloakframe::VideoProcessStatus::Completed);
        assert(result.trackCount == 0);
        assert(result.frameCount >= 55 && result.frameCount <= 65);
        assert(lastPass2Frame == result.frameCount);

        const auto processedInfo = cloakframe::probeVideo(*tools, processedPath, &probeError);
        assert(processedInfo.has_value());
        assert(processedInfo->videoCodec == "h264");
        assert(!processedInfo->audioStreams.empty());
        std::puts("video processor round trip: ok");
    }

    {
        cloakframe::VideoProcessOptions options;
        options.analysisLongEdge = 160;
        options.method = cloakframe::AnonymizationMethod::CustomImage;
        options.customImage = cv::Mat(4, 4, CV_8UC4, cv::Scalar(10, 20, 240, 255));
        options.paddingRatio = 0.0F;
        const QString scaledPath = tempDir.filePath("scaled.mp4");
        std::atomic<bool> cancelled{false};
        bool reviewCalled = false;
        const auto result = cloakframe::processVideo(
            *tools,
            samplePath,
            scaledPath,
            *info,
            options,
            [](const cv::Mat &frame)
            {
                assert(frame.cols == 160 && frame.rows == 120);
                cloakframe::FaceDetections detections;
                detections.push_back({cv::Rect2f(40.0F, 30.0F, 80.0F, 60.0F), 0.9F});
                return detections;
            },
            cancelled,
            {},
            [&](std::vector<cloakframe::Track> &tracks,
                const std::vector<cloakframe::UncoveredSpan> &uncoveredSpans,
                qint64 frameCount,
                const QString &,
                const cloakframe::VideoInfo &)
            {
                reviewCalled = true;
                assert(frameCount >= 55 && frameCount <= 65);
                assert(tracks.size() == 1);
                assert(uncoveredSpans.empty());
                return true;
            });
        assert(result.status == cloakframe::VideoProcessStatus::Completed);
        assert(result.trackCount == 1);
        assert(reviewCalled);

        const auto scaledInfo = cloakframe::probeVideo(*tools, scaledPath, &probeError);
        assert(scaledInfo.has_value());
        cloakframe::VideoFrameReader outputReader;
        assert(outputReader.open(*tools, scaledPath, *scaledInfo));
        cv::Mat outputFrame;
        assert(outputReader.readFrame(outputFrame));
        const cv::Scalar inside = cv::mean(outputFrame(cv::Rect(85, 65, 150, 110)));
        const cv::Scalar outside = cv::mean(outputFrame(cv::Rect(0, 190, 70, 45)));
        assert(inside[2] > 150.0);
        assert(inside[2] > inside[0] * 4.0);
        assert(inside[2] > inside[1] * 4.0);
        assert(outside[0] + outside[1] + outside[2] > 45.0);
        std::puts("custom-image video masking and analysis-to-native coordinate scaling: ok");
    }

    {
        // A hole the tracker refuses to interpolate has to reach the caller as a located
        // range. A bare count leaves a reviewer with nowhere to look.
        cloakframe::VideoProcessOptions options;
        options.analysisLongEdge = 160;
        options.postProcess.maxInterpolationGap = 2;
        options.postProcess.extensionFrames = 0;
        const QString gappedPath = tempDir.filePath("gapped.mp4");
        std::atomic<bool> cancelled{false};
        int detectedFrame = 0;
        bool reviewSawSpan = false;
        const auto result = cloakframe::processVideo(
            *tools,
            samplePath,
            gappedPath,
            *info,
            options,
            [&detectedFrame](const cv::Mat &)
            {
                const int frame = detectedFrame++;
                cloakframe::FaceDetections detections;
                if (frame < 10 || frame >= 16)
                {
                    detections.push_back({cv::Rect2f(40.0F, 30.0F, 80.0F, 60.0F), 0.9F});
                }
                return detections;
            },
            cancelled,
            {},
            [&reviewSawSpan](std::vector<cloakframe::Track> &,
                const std::vector<cloakframe::UncoveredSpan> &uncoveredSpans,
                qint64,
                const QString &,
                const cloakframe::VideoInfo &)
            {
                // The reviewer has to be told where to draw, not only that something is
                // missing.
                reviewSawSpan = uncoveredSpans.size() == 1 && uncoveredSpans[0].firstFrame == 10
                                && uncoveredSpans[0].lastFrame == 15;
                return true;
            });
        assert(result.status == cloakframe::VideoProcessStatus::Completed);
        assert(reviewSawSpan);
        assert(result.uncoveredFrames == 6);
        assert(result.uncoveredSpans.size() == 1);
        assert(result.uncoveredSpans[0].firstFrame == 10);
        assert(result.uncoveredSpans[0].lastFrame == 15);
        assert(result.uncoveredSpans[0].frameCount() == result.uncoveredFrames);
        std::puts("uncovered frame ranges reach the caller: ok");
    }

    {
        const QString reviewCancelledPath = tempDir.filePath("review-cancelled.mp4");
        std::atomic<bool> cancelled{false};
        bool reviewCalled = false;
        const auto result = cloakframe::processVideo(
            *tools,
            samplePath,
            reviewCancelledPath,
            *info,
            {},
            [](const cv::Mat &)
            {
                return cloakframe::FaceDetections{{cv::Rect2f(40.0F, 30.0F, 80.0F, 60.0F), 0.9F}};
            },
            cancelled,
            {},
            [&](std::vector<cloakframe::Track> &,
                const std::vector<cloakframe::UncoveredSpan> &,
                qint64,
                const QString &,
                const cloakframe::VideoInfo &)
            {
                reviewCalled = true;
                return false;
            });
        assert(reviewCalled);
        assert(result.status == cloakframe::VideoProcessStatus::Cancelled);
        assert(!QFile::exists(reviewCancelledPath));
        assert(stagingLeftovers(tempDir.path()) == 0);
        std::puts("video review cancellation: ok");
    }

    {
        const QString replaceableSource = tempDir.filePath("replaceable-source.mp4");
        const QString replacement = tempDir.filePath("replacement.mp4");
        const QString changedSourceOutput = tempDir.filePath("snapshotted-source.mp4");
        assert(QFile::copy(samplePath, replaceableSource));
        assert(generateSample(*tools, replacement, 1));
        const auto replaceableInfo = cloakframe::probeVideo(*tools, replaceableSource, &probeError);
        assert(replaceableInfo.has_value());

        std::atomic<bool> cancelled{false};
        bool reviewCalled = false;
        const auto result = cloakframe::processVideo(*tools,
            replaceableSource,
            changedSourceOutput,
            *replaceableInfo,
            {},
            {},
            cancelled,
            {},
            [&](std::vector<cloakframe::Track> &,
                const std::vector<cloakframe::UncoveredSpan> &,
                qint64,
                const QString &,
                const cloakframe::VideoInfo &)
            {
                reviewCalled = true;
                assert(stagingLeftovers(tempDir.path()) >= 1);
                assert(QFile::remove(replaceableSource));
                assert(QFile::rename(replacement, replaceableSource));
                return true;
            });
        assert(reviewCalled);
        assert(result.status == cloakframe::VideoProcessStatus::Completed);
        assert(result.frameCount >= 55 && result.frameCount <= 65);
        assert(QFile::exists(changedSourceOutput));
        assert(stagingLeftovers(tempDir.path()) == 0);
        std::puts("stable source snapshot between video passes: ok");
    }

    {
        std::atomic<bool> cancelled{true};
        const auto result = cloakframe::processVideo(
            *tools, samplePath, tempDir.filePath("cancelled.mp4"), *info, {}, {}, cancelled);
        assert(result.status == cloakframe::VideoProcessStatus::Cancelled);
        assert(!QFile::exists(tempDir.filePath("cancelled.mp4")));
        std::puts("video processor cancellation: ok");
    }

    {
        const QString pass2CancelledPath = tempDir.filePath("pass2-cancelled.mp4");
        std::atomic<bool> cancelled{false};
        const auto result = cloakframe::processVideo(*tools,
            samplePath,
            pass2CancelledPath,
            *info,
            {},
            {},
            cancelled,
            [&](int pass, qint64 frame, qint64)
            {
                if (pass == 2 && frame == 1)
                {
                    cancelled.store(true, std::memory_order_release);
                }
            });
        assert(result.status == cloakframe::VideoProcessStatus::Cancelled);
        assert(!QFile::exists(pass2CancelledPath));
        std::puts("video pass-2 cancellation cleanup: ok");
    }

    std::puts("all video io tests passed");
    return 0;
}
