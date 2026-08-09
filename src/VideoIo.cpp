#include "cloakframe/VideoIo.hpp"

#include "cloakframe/ImageIo.hpp"
#include "cloakframe/PathUtil.hpp"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalServer>
#include <QLocalSocket>
#include <QProcess>
#include <QRandomGenerator>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QThread>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <limits>
#include <mutex>

namespace cloakframe
{
    namespace
    {
        constexpr int kProcessStartTimeoutMs = 15000;
        constexpr int kProcessStopTimeoutMs = 2000;
        constexpr int kProcessIoTimeoutMs = 60000;
        constexpr int kProcessFinishTimeoutMs = 900000;
#if defined(_WIN32)
        constexpr qint64 kDecodeBufferBytes = 64LL * 1024 * 1024;
#endif
        constexpr qint64 kMaxEncodeBacklogBytes = 16LL * 1024 * 1024;

        QString trVideo(const char *text)
        {
            return QCoreApplication::translate("cloakframe::VideoIo", text);
        }

        QString executableName(const QString &base)
        {
#ifdef _WIN32
            return base + ".exe";
#else
            return base;
#endif
        }

        bool verifyBundledChecksum(const QString &toolPath, QString *error)
        {
            const QString manifestPath = toolPath + ".sha256";
            if (!QFileInfo::exists(manifestPath))
            {
                return true;
            }

            QFile manifest(manifestPath);
            if (!manifest.open(QIODevice::ReadOnly))
            {
                if (error)
                {
                    *error = trVideo(QT_TRANSLATE_NOOP(
                        "cloakframe::VideoIo", "Could not read the FFmpeg checksum manifest."));
                }
                return false;
            }
            const QString expected =
                QString::fromLatin1(manifest.readAll()).trimmed().section(' ', 0, 0);

            QFile tool(toolPath);
            if (!tool.open(QIODevice::ReadOnly))
            {
                if (error)
                {
                    *error = trVideo(QT_TRANSLATE_NOOP(
                        "cloakframe::VideoIo", "Could not read the bundled FFmpeg binary."));
                }
                return false;
            }
            QCryptographicHash hash(QCryptographicHash::Sha256);
            if (!hash.addData(&tool))
            {
                if (error)
                {
                    *error = trVideo(QT_TRANSLATE_NOOP(
                        "cloakframe::VideoIo", "Could not read the bundled FFmpeg binary."));
                }
                return false;
            }
            const QString actual = QString::fromLatin1(hash.result().toHex());
            if (actual.compare(expected, Qt::CaseInsensitive) != 0)
            {
                if (error)
                {
                    *error = trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo",
                        "The bundled FFmpeg binary failed its integrity check."));
                }
                return false;
            }
            return true;
        }

        struct LocatedTool
        {
            QString path;
            bool bundled = false;
        };

        LocatedTool locateTool(const QString &baseName)
        {
            const QString exe = executableName(baseName);
            if (QCoreApplication::instance() != nullptr)
            {
                const auto appDir = QCoreApplication::applicationDirPath();
                const std::array<QString, 3> candidates = {
                    appDir + "/" + exe,
                    appDir + "/ffmpeg/" + exe,
                    appDir + "/../Resources/ffmpeg/" + exe,
                };
                for (const auto &candidate : candidates)
                {
                    const QFileInfo info(QDir::cleanPath(candidate));
                    if (info.exists() && info.isFile() && info.isExecutable())
                    {
                        return {info.absoluteFilePath(), true};
                    }
                }
            }

            QString found = QStandardPaths::findExecutable(exe);
            if (found.isEmpty())
            {
#if defined(Q_OS_MACOS)
                const QStringList fallbackDirs = {
                    "/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"};
#elif defined(Q_OS_UNIX)
                const QStringList fallbackDirs = {"/usr/local/bin", "/usr/bin"};
#else
                const QStringList fallbackDirs;
#endif
                if (!fallbackDirs.isEmpty())
                {
                    found = QStandardPaths::findExecutable(exe, fallbackDirs);
                }
            }
            return {found, false};
        }

        QString probeVersionLine(const QString &ffmpegPath)
        {
            QProcess process;
            process.start(ffmpegPath, {"-version"});
            if (!process.waitForStarted(kProcessStartTimeoutMs)
                || !process.waitForFinished(kProcessStartTimeoutMs))
            {
                process.kill();
                return {};
            }
            if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
            {
                return {};
            }
            const QString output = QString::fromUtf8(process.readAllStandardOutput());
            return output.section('\n', 0, 0).trimmed();
        }

        bool parseRational(const QString &value, int &num, int &den)
        {
            // ffprobe writes frame rates as "30/1" but aspect ratios as "16:15".
            const auto parts = value.split(value.contains(':') ? ':' : '/');
            bool okNum = false;
            bool okDen = true;
            int parsedNum = parts.value(0).toInt(&okNum);
            int parsedDen = 1;
            if (parts.size() > 1)
            {
                parsedDen = parts.value(1).toInt(&okDen);
            }
            if (!okNum || !okDen || parsedNum <= 0 || parsedDen <= 0)
            {
                return false;
            }
            num = parsedNum;
            den = parsedDen;
            return true;
        }

        int normalizedRotation(double rotation)
        {
            int value = static_cast<int>(std::lround(rotation)) % 360;
            if (value < 0)
            {
                value += 360;
            }
            return value;
        }

        QString processErrorDetail(QProcess &process)
        {
            const QString stderrText = QString::fromUtf8(process.readAllStandardError()).trimmed();
            if (!stderrText.isEmpty())
            {
                const auto lines = stderrText.split('\n', Qt::SkipEmptyParts);
                return lines.mid(std::max(0, static_cast<int>(lines.size()) - 3))
                    .join(' ')
                    .trimmed();
            }
            return process.errorString();
        }

        // Only a plain ISO 639 code is carried over. Anything else is container text of unknown
        // provenance and is dropped with the rest of the metadata.
        QString sanitizedLanguageTag(const QJsonValue &value)
        {
            const QString tag = value.toString();
            if (tag.size() < 2 || tag.size() > 3 || tag == QLatin1String("und"))
            {
                return {};
            }
            const bool lettersOnly = std::all_of(tag.cbegin(),
                tag.cend(),
                [](const QChar character)
                {
                    return character >= QLatin1Char('a') && character <= QLatin1Char('z');
                });
            return lettersOnly ? tag : QString();
        }

        QString codecPrefix(const VideoCodec codec)
        {
            return codec == VideoCodec::Hevc ? QStringLiteral("hevc") : QStringLiteral("h264");
        }

        QString softwareEncoderName(const VideoCodec codec)
        {
            return codec == VideoCodec::Hevc ? QStringLiteral("libx265")
                                             : QStringLiteral("libx264");
        }

        QStringList hardwareEncoderCandidates(const VideoCodec codec)
        {
            const QString prefix = codecPrefix(codec);
#if defined(_WIN32) || defined(__linux__)
            return {prefix + QStringLiteral("_nvenc"), prefix + QStringLiteral("_qsv")};
#elif defined(__APPLE__)
            return {prefix + QStringLiteral("_videotoolbox")};
#else
            Q_UNUSED(prefix);
            return {};
#endif
        }

        QStringList videoEncoderArgs(const QString &encoder, const int crf)
        {
            const bool hevc =
                encoder.startsWith(QLatin1String("hevc_")) || encoder == QLatin1String("libx265");
            const int codecCrf = hevc ? crf + 5 : crf;
            if (encoder.endsWith(QLatin1String("_nvenc")))
            {
                return {"-c:v",
                    encoder,
                    "-preset",
                    "p6",
                    "-rc",
                    "vbr",
                    "-cq",
                    QString::number(codecCrf),
                    "-b:v",
                    "0"};
            }
            if (encoder.endsWith(QLatin1String("_qsv")))
            {
                return {"-c:v",
                    encoder,
                    "-preset",
                    "slow",
                    "-global_quality",
                    QString::number(codecCrf)};
            }
            if (encoder.endsWith(QLatin1String("_videotoolbox")))
            {
                return {
                    "-c:v", encoder, "-q:v", QString::number(std::clamp(100 - 2 * crf, 1, 100))};
            }
            return {"-c:v", encoder, "-preset", "medium", "-crf", QString::number(codecCrf)};
        }

        bool encoderWorks(const FfmpegTools &tools,
            const QString &encoder,
            const int width,
            const int height,
            const int fpsNum,
            const int fpsDen)
        {
            QStringList arguments = {
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                QString("color=black:size=%1x%2:rate=%3/%4")
                    .arg(width)
                    .arg(height)
                    .arg(fpsNum)
                    .arg(fpsDen),
                "-frames:v",
                "3",
                "-pix_fmt",
                "yuv420p",
            };
            arguments << videoEncoderArgs(encoder, crfForQuality(VideoQuality::Balanced)) << "-f"
                      << "null" << "-";

            QProcess probe;
            probe.start(tools.ffmpegPath, arguments);
            if (!probe.waitForStarted(kProcessStartTimeoutMs))
            {
                return false;
            }
            if (!probe.waitForFinished(kProcessIoTimeoutMs))
            {
                probe.kill();
                probe.waitForFinished(kProcessStartTimeoutMs);
                return false;
            }
            return probe.exitStatus() == QProcess::NormalExit && probe.exitCode() == 0;
        }

        QString selectVideoEncoder(const FfmpegTools &tools,
            const VideoCodec codec,
            const int width,
            const int height,
            const int fpsNum,
            const int fpsDen)
        {
            static std::mutex cacheMutex;
            static QHash<QString, bool> cache;

            std::lock_guard lock(cacheMutex);
            for (const auto &encoder : hardwareEncoderCandidates(codec))
            {
                const QString key = QString("%1:%2x%3:%4/%5")
                                        .arg(encoder)
                                        .arg(width)
                                        .arg(height)
                                        .arg(fpsNum)
                                        .arg(fpsDen);
                auto it = cache.find(key);
                if (it == cache.end())
                {
                    it = cache.insert(
                        key, encoderWorks(tools, encoder, width, height, fpsNum, fpsDen));
                    spdlog::info("Hardware video encoder {} {} at {}x{} {}/{} fps",
                        encoder.toStdString(),
                        it.value() ? "available" : "unavailable",
                        width,
                        height,
                        fpsNum,
                        fpsDen);
                }
                if (it.value())
                {
                    return encoder;
                }
            }
            return softwareEncoderName(codec);
        }
    }

    std::optional<FfmpegTools> locateFfmpegTools(QString *error)
    {
        static std::mutex cacheMutex;
        static std::optional<FfmpegTools> cached;

        std::lock_guard lock(cacheMutex);
        if (cached)
        {
            return cached;
        }

        const auto fail = [&](const QString &message) -> std::optional<FfmpegTools>
        {
            if (error)
            {
                *error = message;
            }
            return std::nullopt;
        };

        const auto ffmpeg = locateTool("ffmpeg");
        const auto ffprobe = locateTool("ffprobe");
        if (ffmpeg.path.isEmpty() || ffprobe.path.isEmpty())
        {
            return fail(trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "FFmpeg was not found. Video processing is unavailable.")));
        }

        QString checksumError;
        if (ffmpeg.bundled && !verifyBundledChecksum(ffmpeg.path, &checksumError))
        {
            return fail(checksumError);
        }
        if (ffprobe.bundled && !verifyBundledChecksum(ffprobe.path, &checksumError))
        {
            return fail(checksumError);
        }

        const QString version = probeVersionLine(ffmpeg.path);
        if (version.isEmpty())
        {
            return fail(trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "FFmpeg was found but could not be executed.")));
        }

        FfmpegTools tools;
        tools.ffmpegPath = ffmpeg.path;
        tools.ffprobePath = ffprobe.path;
        tools.bundled = ffmpeg.bundled;
        tools.versionLine = version;
        spdlog::info(
            "FFmpeg: {} ({})", version.toStdString(), tools.bundled ? "bundled" : "system");
        cached = tools;
        return cached;
    }

    bool isSupportedVideo(const std::filesystem::path &path)
    {
        auto extension = pathToUtf8(path.extension());
        std::ranges::transform(extension,
            extension.begin(),
            [](unsigned char ch)
            {
                return static_cast<char>(std::tolower(ch));
            });
        return extension == ".mp4" || extension == ".mov" || extension == ".m4v"
               || extension == ".webm";
    }

    int crfForQuality(const VideoQuality quality)
    {
        switch (quality)
        {
        case VideoQuality::Balanced:
            return 21;
        case VideoQuality::SpaceSaver:
            return 24;
        case VideoQuality::HighQuality:
            break;
        }
        return 18;
    }

    int VideoInfo::displayWidth() const
    {
        return (rotation == 90 || rotation == 270) ? height : width;
    }

    int VideoInfo::displayHeight() const
    {
        return (rotation == 90 || rotation == 270) ? width : height;
    }

    double VideoInfo::fps() const
    {
        return fpsDen > 0 ? static_cast<double>(fpsNum) / fpsDen : 0.0;
    }

    std::optional<VideoInfo> probeVideo(
        const FfmpegTools &tools, const QString &path, QString *error)
    {
        QProcess process;
        process.start(tools.ffprobePath,
            {"-v", "error", "-print_format", "json", "-show_streams", "-show_format", path});
        if (!process.waitForStarted(kProcessStartTimeoutMs)
            || !process.waitForFinished(kProcessIoTimeoutMs))
        {
            process.kill();
            if (error)
            {
                *error = trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo",
                    "Could not inspect the video (ffprobe did not respond)."));
            }
            return std::nullopt;
        }
        if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
        {
            if (error)
            {
                *error = trVideo(
                    QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Could not inspect the video: %1"))
                             .arg(processErrorDetail(process));
            }
            return std::nullopt;
        }

        const auto document = QJsonDocument::fromJson(process.readAllStandardOutput());
        const auto streams = document.object().value("streams").toArray();
        const auto format = document.object().value("format").toObject();

        VideoInfo info;
        bool videoFound = false;
        for (const auto &entry : streams)
        {
            const auto stream = entry.toObject();
            const auto type = stream.value("codec_type").toString();
            if (type == "audio")
            {
                AudioStreamInfo audio;
                audio.codec = stream.value("codec_name").toString();
                audio.language =
                    sanitizedLanguageTag(stream.value("tags").toObject().value("language"));
                info.audioStreams.push_back(std::move(audio));
                continue;
            }
            if (type != "video" || videoFound)
            {
                continue;
            }
            videoFound = true;

            info.videoCodec = stream.value("codec_name").toString();
            info.pixelFormat = stream.value("pix_fmt").toString();
            info.colorTransfer = stream.value("color_transfer").toString();
            info.width = stream.value("width").toInt();
            info.height = stream.value("height").toInt();

            bool startOk = false;
            const double streamStart = stream.value("start_time").toString().toDouble(&startOk);
            if (startOk && std::isfinite(streamStart) && streamStart > 0.0)
            {
                info.startTimeSeconds = streamStart;
            }
            int parsedSarNum = 0;
            int parsedSarDen = 0;
            if (parseRational(
                    stream.value("sample_aspect_ratio").toString(), parsedSarNum, parsedSarDen)
                && parsedSarNum > 0 && parsedSarDen > 0)
            {
                info.sarNum = parsedSarNum;
                info.sarDen = parsedSarDen;
            }

            int avgNum = 0;
            int avgDen = 1;
            const bool avgValid =
                parseRational(stream.value("avg_frame_rate").toString(), avgNum, avgDen);
            int rNum = 0;
            int rDen = 1;
            const bool rValid = parseRational(stream.value("r_frame_rate").toString(), rNum, rDen);
            if (avgValid)
            {
                info.fpsNum = avgNum;
                info.fpsDen = avgDen;
            }
            else if (rValid)
            {
                info.fpsNum = rNum;
                info.fpsDen = rDen;
            }
            info.isVfr =
                avgValid && rValid
                && static_cast<qint64>(avgNum) * rDen != static_cast<qint64>(rNum) * avgDen;

            for (const auto &sideDataEntry : stream.value("side_data_list").toArray())
            {
                const auto sideData = sideDataEntry.toObject();
                if (sideData.contains("rotation"))
                {
                    info.rotation = normalizedRotation(sideData.value("rotation").toDouble());
                }
            }
            const auto rotateTag = stream.value("tags").toObject().value("rotate").toString();
            if (info.rotation == 0 && !rotateTag.isEmpty())
            {
                info.rotation = normalizedRotation(rotateTag.toDouble());
            }

            bool durationOk = false;
            const double streamDuration = stream.value("duration").toString().toDouble(&durationOk);
            if (durationOk && streamDuration > 0)
            {
                info.durationSeconds = streamDuration;
            }
            const auto frames = stream.value("nb_frames").toString().toLongLong();
            if (frames > 0)
            {
                info.estimatedFrameCount = frames;
            }
        }

        if (info.durationSeconds <= 0)
        {
            info.durationSeconds = format.value("duration").toString().toDouble();
        }
        if (info.estimatedFrameCount <= 0 && info.durationSeconds > 0 && info.fps() > 0)
        {
            const double estimate = info.durationSeconds * info.fps();
            if (std::isfinite(estimate) && estimate > 0.0)
            {
                info.estimatedFrameCount =
                    estimate >= static_cast<double>(std::numeric_limits<qint64>::max())
                        ? std::numeric_limits<qint64>::max()
                        : static_cast<qint64>(std::llround(estimate));
            }
        }

        if (!videoFound)
        {
            if (error)
            {
                *error = trVideo(
                    QT_TRANSLATE_NOOP("cloakframe::VideoIo", "The file contains no video stream."));
            }
            return std::nullopt;
        }
        return info;
    }

    QString videoUnsupportedReason(const VideoInfo &info)
    {
        if (info.width <= 0 || info.height <= 0 || info.fpsNum <= 0 || info.fpsDen <= 0)
        {
            return trVideo(
                QT_TRANSLATE_NOOP("cloakframe::VideoIo", "the video stream could not be read"));
        }
        const qint64 pixelCount = static_cast<qint64>(info.width) * info.height;
        if (info.width > kMaxVideoDimension || info.height > kMaxVideoDimension
            || pixelCount > kMaxVideoPixelCount)
        {
            return trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "the video resolution exceeds the safety limit"));
        }
        const double frameRate = info.fps();
        if (!std::isfinite(frameRate) || frameRate > kMaxVideoFrameRate)
        {
            return trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "the video frame rate exceeds the safety limit"));
        }
        if (!std::isfinite(info.durationSeconds) || info.durationSeconds < 0.0
            || info.durationSeconds > kMaxVideoDurationSeconds)
        {
            return trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "the video duration exceeds the safety limit"));
        }
        if (info.estimatedFrameCount < 0 || info.estimatedFrameCount > kMaxVideoFrameCount)
        {
            return trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "the video frame count exceeds the safety limit"));
        }
        if (info.videoCodec != "h264" && info.videoCodec != "hevc" && info.videoCodec != "vp8"
            && info.videoCodec != "vp9")
        {
            return trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo",
                               "unsupported video codec '%1' (H.264, HEVC, VP8 and VP9 only)"))
                .arg(info.videoCodec);
        }
        static const QRegularExpression highBitDepth("(9|10|12|14|16)(le|be)?$");
        if (highBitDepth.match(info.pixelFormat).hasMatch())
        {
            return trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "10-bit or higher bit depth is not supported yet"));
        }
        if (info.colorTransfer == "smpte2084" || info.colorTransfer == "arib-std-b67")
        {
            return trVideo(
                QT_TRANSLATE_NOOP("cloakframe::VideoIo", "HDR video is not supported yet"));
        }
        return {};
    }

    VideoFrameReader::VideoFrameReader() = default;

    VideoFrameReader::~VideoFrameReader()
    {
        close();
    }

    bool VideoFrameReader::open(const FfmpegTools &tools,
        const QString &path,
        const VideoInfo &info,
        const int decodeLongEdge)
    {
        close();
        error_.clear();
        atEnd_ = false;
        frameWidth_ = info.displayWidth();
        frameHeight_ = info.displayHeight();
        if (frameWidth_ <= 0 || frameHeight_ <= 0 || info.fpsNum <= 0 || info.fpsDen <= 0)
        {
            error_ = trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Invalid video dimensions."));
            return false;
        }

        // Rebase to the first video frame so frame 0 is that frame. Without this a stream
        // that starts after the container origin is padded with duplicates and every frame
        // index shifts relative to the review preview.
        QString filter = QString("setpts=PTS-STARTPTS,fps=fps=%1/%2:start_time=0")
                             .arg(info.fpsNum)
                             .arg(info.fpsDen);
        const int longEdge = std::max(frameWidth_, frameHeight_);
        if (decodeLongEdge > 0 && decodeLongEdge < longEdge)
        {
            const double scale = static_cast<double>(decodeLongEdge) / longEdge;
            frameWidth_ = std::max(2, static_cast<int>(std::lround(frameWidth_ * scale)));
            frameHeight_ = std::max(2, static_cast<int>(std::lround(frameHeight_ * scale)));
            filter += QString(",scale=%1:%2:flags=area").arg(frameWidth_).arg(frameHeight_);
        }

        QString sink = QStringLiteral("-");
#if defined(_WIN32)
        server_ = std::make_unique<QLocalServer>();
        const QString pipeName = QString("cloakframe-video-%1-%2")
                                     .arg(QCoreApplication::applicationPid())
                                     .arg(QRandomGenerator::global()->generate64(), 0, 16);
        if (!server_->listen(pipeName))
        {
            error_ = trVideo(
                QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Could not start FFmpeg for decoding."));
            server_.reset();
            return false;
        }
        sink = QStringLiteral(R"(\\.\pipe\)") + pipeName;
#endif

        process_ = std::make_unique<QProcess>();
        process_->start(tools.ffmpegPath,
            {"-v",
                "error",
                "-nostdin",
                "-y",
                // A damaged stream must not decode into a silently shorter video. `-xerror`
                // turns a recovered decode error into a nonzero exit, which the read loop
                // reports instead of publishing whatever frames survived.
                "-xerror",
                "-i",
                path,
                "-map",
                "0:v:0",
                "-vf",
                filter,
                "-f",
                "rawvideo",
                "-pix_fmt",
                "bgr24",
                sink});
        if (!process_->waitForStarted(kProcessStartTimeoutMs))
        {
            error_ = trVideo(
                QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Could not start FFmpeg for decoding."));
            process_.reset();
            server_.reset();
            return false;
        }
#if defined(_WIN32)
        bool connected = false;
        QElapsedTimer connectTimer;
        connectTimer.start();
        while (connectTimer.elapsed() < kProcessIoTimeoutMs)
        {
            if (server_->waitForNewConnection(100))
            {
                connected = true;
                break;
            }
            if (process_->state() == QProcess::NotRunning)
            {
                connected = server_->waitForNewConnection(0);
                break;
            }
        }
        socket_.reset(connected ? server_->nextPendingConnection() : nullptr);
        if (!socket_)
        {
            process_->waitForFinished(kProcessStartTimeoutMs);
            error_ = trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Decoding failed: %1"))
                         .arg(processErrorDetail(*process_));
            close();
            return false;
        }
        socket_->setParent(nullptr);
        socket_->setReadBufferSize(kDecodeBufferBytes);
        server_->close();
#endif
        return true;
    }

    int VideoFrameReader::frameWidth() const
    {
        return frameWidth_;
    }

    int VideoFrameReader::frameHeight() const
    {
        return frameHeight_;
    }

    bool VideoFrameReader::readFrame(cv::Mat &frame, const std::function<bool()> &continueGuard)
    {
        if (!process_ || atEnd_)
        {
            return false;
        }

        QIODevice *device = socket_ ? static_cast<QIODevice *>(socket_.get())
                                    : static_cast<QIODevice *>(process_.get());
        const auto exhausted = [this]() -> bool
        {
            if (socket_)
            {
                return socket_->state() == QLocalSocket::UnconnectedState
                       && socket_->bytesAvailable() == 0;
            }
            return process_->state() == QProcess::NotRunning && process_->bytesAvailable() == 0;
        };

        const qint64 frameBytes = static_cast<qint64>(frameWidth_) * frameHeight_ * 3;
        frame.create(frameHeight_, frameWidth_, CV_8UC3);

        qint64 received = 0;
        while (received < frameBytes)
        {
            if (continueGuard && !continueGuard())
            {
                atEnd_ = true;
                return false;
            }
            const qint64 chunk = device->read(
                reinterpret_cast<char *>(frame.data) + received, frameBytes - received);
            if (chunk > 0)
            {
                received += chunk;
                continue;
            }
            if (chunk < 0 || exhausted())
            {
                atEnd_ = true;
                process_->waitForFinished(kProcessStartTimeoutMs);
                if (received > 0)
                {
                    error_ = trVideo(
                        QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Decoding ended mid-frame: %1"))
                                 .arg(processErrorDetail(*process_));
                }
                else if (process_->exitStatus() != QProcess::NormalExit
                         || process_->exitCode() != 0)
                {
                    error_ =
                        trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Decoding failed: %1"))
                            .arg(processErrorDetail(*process_));
                }
                return false;
            }
            QElapsedTimer waitTimer;
            waitTimer.start();
            bool ready = false;
            while (!ready && !exhausted())
            {
                if (continueGuard && !continueGuard())
                {
                    atEnd_ = true;
                    return false;
                }
                const qint64 remaining = kProcessIoTimeoutMs - waitTimer.elapsed();
                if (remaining <= 0)
                {
                    error_ =
                        trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Decoding timed out."));
                    atEnd_ = true;
                    return false;
                }
                ready =
                    device->waitForReadyRead(static_cast<int>(std::min<qint64>(remaining, 100)));
            }
        }
        return true;
    }

    void VideoFrameReader::close()
    {
        if (process_)
        {
            if (process_->state() != QProcess::NotRunning)
            {
                process_->kill();
                process_->waitForFinished(kProcessStopTimeoutMs);
            }
            process_.reset();
        }
        socket_.reset();
        server_.reset();
        atEnd_ = true;
    }

    bool VideoFrameReader::atEnd() const
    {
        return atEnd_;
    }

    QString VideoFrameReader::errorString() const
    {
        return error_;
    }

    VideoFrameWriter::VideoFrameWriter() = default;

    VideoFrameWriter::~VideoFrameWriter()
    {
        abort();
    }

    bool VideoFrameWriter::open(const FfmpegTools &tools,
        const QString &destination,
        const QString &audioSource,
        const VideoInfo &info,
        const int crf,
        const bool hardwareEncoder,
        const VideoCodec codec,
        const QString &outputRoot,
        const QString &relativeDestination)
    {
        abort();
        error_.clear();
        encoderFailed_ = false;
        frameWidth_ = info.displayWidth();
        frameHeight_ = info.displayHeight();
        if (frameWidth_ <= 0 || frameHeight_ <= 0 || info.fpsNum <= 0 || info.fpsDen <= 0)
        {
            error_ = trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Invalid video dimensions."));
            return false;
        }

        const QString softwareEncoder = softwareEncoderName(codec);
        encoderName_ = softwareEncoder;
        if (hardwareEncoder)
        {
            encoderName_ = selectVideoEncoder(tools,
                codec,
                frameWidth_ - (frameWidth_ % 2),
                frameHeight_ - (frameHeight_ % 2),
                info.fpsNum,
                info.fpsDen);
        }
        usedHardwareEncoder_ = encoderName_ != softwareEncoder;
        spdlog::info("Video encoder: {}", encoderName_.toStdString());

        destinationPath_ = destination;
        outputRootPath_ = outputRoot;
        relativeDestinationPath_ = relativeDestination;
        const QString stagingBase = !outputRootPath_.isEmpty()
                                        ? outputRootPath_
                                        : QFileInfo(destinationPath_).absolutePath();
        stagingDirectory_ = std::make_unique<QTemporaryDir>(
            QDir(stagingBase).filePath(QStringLiteral(".cloakframe-encode-XXXXXX")));
        if (!stagingDirectory_->isValid())
        {
            stagingDirectory_.reset();
            error_ = trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "Could not create a temporary directory for encoding."));
            return false;
        }
        tempPath_ = stagingDirectory_->filePath(QStringLiteral("video.mp4"));

        static const QStringList mp4CompatibleAudio = {"aac", "mp3", "ac3", "eac3", "alac"};
        const bool hasAudio = !info.audioStreams.empty();

        QStringList arguments = {
            "-v",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgr24",
            "-s",
            QString("%1x%2").arg(frameWidth_).arg(frameHeight_),
            "-framerate",
            QString("%1/%2").arg(info.fpsNum).arg(info.fpsDen),
            "-i",
            "-",
        };
        if (hasAudio)
        {
            // The encoded video covers the video stream's own interval, so the audio has to be
            // taken from that same interval rather than from the container origin.
            if (info.startTimeSeconds > 0)
            {
                arguments << "-ss" << QString::number(info.startTimeSeconds, 'f', 6);
            }
            if (info.durationSeconds > 0)
            {
                arguments << "-t" << QString::number(info.durationSeconds, 'f', 3);
            }
        }
        arguments << "-i" << audioSource << "-map" << "0:v:0";
        if (hasAudio)
        {
            // Every audio stream, not just the first: commentary, alternate languages and
            // descriptive tracks must survive anonymization.
            arguments << "-map" << "1:a?";
        }
        arguments << videoEncoderArgs(encoderName_, crf) << "-pix_fmt" << "yuv420p";
        if (codec == VideoCodec::Hevc)
        {
            arguments << "-tag:v" << "hvc1";
        }
        QStringList videoFilters;
        if ((frameWidth_ % 2) != 0 || (frameHeight_ % 2) != 0)
        {
            videoFilters << QString("crop=%1:%2:0:0")
                                .arg(frameWidth_ - (frameWidth_ % 2))
                                .arg(frameHeight_ - (frameHeight_ % 2));
        }
        // The raw frames carry no aspect metadata, so an anamorphic source would otherwise be
        // published as square-pixel and display at the wrong shape.
        if (info.sarNum > 0 && info.sarDen > 0 && info.sarNum != info.sarDen)
        {
            videoFilters << QString("setsar=%1/%2").arg(info.sarNum).arg(info.sarDen);
        }
        if (!videoFilters.isEmpty())
        {
            arguments << "-vf" << videoFilters.join(QLatin1Char(','));
        }
        for (std::size_t stream = 0; stream < info.audioStreams.size(); ++stream)
        {
            // Output audio stream `stream` is input audio stream `stream`, because every one of
            // them is mapped in order. Only the streams MP4 cannot carry are re-encoded.
            const auto &audio = info.audioStreams[stream];
            const auto index = static_cast<int>(stream);
            if (mp4CompatibleAudio.contains(audio.codec))
            {
                arguments << QString("-c:a:%1").arg(index) << "copy";
            }
            else
            {
                arguments << QString("-c:a:%1").arg(index) << "aac" << QString("-b:a:%1").arg(index)
                          << "192k";
            }
            if (!audio.language.isEmpty())
            {
                arguments << QString("-metadata:s:a:%1").arg(index)
                          << QString("language=%1").arg(audio.language);
            }
        }
        arguments << "-map_metadata" << "-1"
                  << "-map_chapters" << "-1"
                  << "-movflags" << "+faststart"
                  << "-f" << "mp4" << tempPath_;

        process_ = std::make_unique<QProcess>();
        process_->start(tools.ffmpegPath, arguments);
        if (!process_->waitForStarted(kProcessStartTimeoutMs))
        {
            noteEncoderFailure(trVideo(
                QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Could not start FFmpeg for encoding.")));
            process_.reset();
            releaseStaging();
            return false;
        }
        return true;
    }

    bool VideoFrameWriter::writeFrame(
        const cv::Mat &frame, const std::function<bool()> &continueGuard)
    {
        if (continueGuard && !continueGuard())
        {
            return false;
        }
        if (!process_ || process_->state() != QProcess::Running)
        {
            noteEncoderFailure(
                trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Encoding failed: %1"))
                    .arg(process_ ? processErrorDetail(*process_) : QString()));
            return false;
        }
        if (frame.cols != frameWidth_ || frame.rows != frameHeight_ || frame.type() != CV_8UC3)
        {
            error_ = trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "Internal error: frame does not match the video format."));
            return false;
        }

        cv::Mat continuous = frame;
        if (!frame.isContinuous())
        {
            continuous = frame.clone();
        }

        const qint64 frameBytes = static_cast<qint64>(frameWidth_) * frameHeight_ * 3;
        qint64 written = 0;
        while (written < frameBytes)
        {
            if (continueGuard && !continueGuard())
            {
                return false;
            }
            const qint64 chunk = process_->write(
                reinterpret_cast<const char *>(continuous.data) + written, frameBytes - written);
            if (chunk < 0 || process_->state() != QProcess::Running)
            {
                noteEncoderFailure(
                    trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Encoding failed: %1"))
                        .arg(processErrorDetail(*process_)));
                return false;
            }
            written += chunk;
            while (process_->bytesToWrite() > kMaxEncodeBacklogBytes)
            {
                QElapsedTimer waitTimer;
                waitTimer.start();
                bool writtenToProcess = false;
                while (!writtenToProcess && process_->bytesToWrite() > kMaxEncodeBacklogBytes)
                {
                    if (continueGuard && !continueGuard())
                    {
                        return false;
                    }
                    if (process_->state() != QProcess::Running)
                    {
                        noteEncoderFailure(
                            trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Encoding failed: %1"))
                                .arg(processErrorDetail(*process_)));
                        return false;
                    }
                    const qint64 remaining = kProcessIoTimeoutMs - waitTimer.elapsed();
                    if (remaining <= 0)
                    {
                        noteEncoderFailure(
                            trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Encoding failed: %1"))
                                .arg(processErrorDetail(*process_)));
                        return false;
                    }
                    writtenToProcess = process_->waitForBytesWritten(
                        static_cast<int>(std::min<qint64>(remaining, 100)));
                }
            }
        }
        return true;
    }

    bool VideoFrameWriter::finish(const std::function<bool()> &publishGuard)
    {
        if (!process_)
        {
            return false;
        }

        process_->closeWriteChannel();
        QElapsedTimer finishTimer;
        finishTimer.start();
        while (process_->state() != QProcess::NotRunning)
        {
            if (publishGuard && !publishGuard())
            {
                error_ = trVideo(QT_TRANSLATE_NOOP(
                    "cloakframe::VideoIo", "The source video changed during processing."));
                abort();
                return false;
            }
            const qint64 remaining = kProcessFinishTimeoutMs - finishTimer.elapsed();
            if (remaining <= 0)
            {
                noteEncoderFailure(trVideo(QT_TRANSLATE_NOOP(
                    "cloakframe::VideoIo", "Encoding timed out while finalizing.")));
                abort();
                return false;
            }
            process_->waitForFinished(static_cast<int>(std::min<qint64>(remaining, 100)));
        }
        const bool ok = process_->exitStatus() == QProcess::NormalExit && process_->exitCode() == 0;
        if (!ok)
        {
            noteEncoderFailure(
                trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo", "Encoding failed: %1"))
                    .arg(processErrorDetail(*process_)));
            abort();
            return false;
        }
        process_.reset();

        if (publishGuard && !publishGuard())
        {
            releaseStaging();
            error_ = trVideo(QT_TRANSLATE_NOOP(
                "cloakframe::VideoIo", "The source video changed during processing."));
            return false;
        }

        std::filesystem::path outputRoot;
        std::filesystem::path relativeDestination;
        if (!outputRootPath_.isEmpty() && !relativeDestinationPath_.isEmpty())
        {
            outputRoot = pathFromQString(outputRootPath_);
            relativeDestination = pathFromQString(relativeDestinationPath_);
        }
        else
        {
            std::error_code ec;
            const auto destination =
                std::filesystem::absolute(pathFromQString(destinationPath_), ec);
            if (!ec)
            {
                outputRoot = std::filesystem::canonical(destination.parent_path(), ec);
                if (!ec)
                {
                    relativeDestination = destination.filename();
                }
            }
        }

        bool guardAccepted = true;
        const auto finalGuard = [&]
        {
            guardAccepted = !publishGuard || publishGuard();
            return guardAccepted;
        };
        bool published = false;
        if (!outputRoot.empty() && !relativeDestination.empty())
        {
            const auto moveResult = moveFileNoReplaceAtRoot(
                pathFromQString(tempPath_), outputRoot, relativeDestination, finalGuard);
            published = moveResult == FileMoveResult::Moved;
            if (!published && guardAccepted && !QFileInfo::exists(destinationPath_))
            {
                published = copyFileNoReplaceAtRoot(
                    pathFromQString(tempPath_), outputRoot, relativeDestination, finalGuard);
            }
        }
        if (!published)
        {
            releaseStaging();
            if (!guardAccepted)
            {
                error_ = trVideo(QT_TRANSLATE_NOOP(
                    "cloakframe::VideoIo", "The source video changed during processing."));
            }
            else
            {
                error_ = QFileInfo::exists(destinationPath_)
                             ? trVideo(QT_TRANSLATE_NOOP(
                                   "cloakframe::VideoIo", "The output file already exists."))
                             : trVideo(QT_TRANSLATE_NOOP("cloakframe::VideoIo",
                                   "Could not move the finished video into place."));
            }
            return false;
        }
        releaseStaging();
        return true;
    }

    void VideoFrameWriter::releaseStaging()
    {
        if (stagingDirectory_)
        {
            for (int attempt = 0;
                stagingDirectory_->isValid() && !stagingDirectory_->remove() && attempt < 20;
                ++attempt)
            {
                QThread::msleep(100);
            }
            stagingDirectory_.reset();
        }
        tempPath_.clear();
    }

    void VideoFrameWriter::abort()
    {
        if (process_)
        {
            if (process_->state() != QProcess::NotRunning)
            {
                process_->kill();
                process_->waitForFinished(kProcessStopTimeoutMs);
            }
            process_.reset();
        }
        releaseStaging();
        outputRootPath_.clear();
        relativeDestinationPath_.clear();
    }

    QString VideoFrameWriter::errorString() const
    {
        return error_;
    }

    QString VideoFrameWriter::encoderName() const
    {
        return encoderName_;
    }

    bool VideoFrameWriter::usedHardwareEncoder() const
    {
        return usedHardwareEncoder_;
    }

    bool VideoFrameWriter::encoderFailed() const
    {
        return encoderFailed_;
    }

    void VideoFrameWriter::noteEncoderFailure(const QString &message)
    {
        encoderFailed_ = true;
        error_ = message;
    }
}
