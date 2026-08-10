#pragma once

#include <QString>
#include <QStringList>

#include <opencv2/core.hpp>

#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <vector>

class QLocalServer;
class QLocalSocket;
class QProcess;

namespace cloakframe
{
    class StageDirectory;

    inline constexpr int kMaxVideoDimension = 16'384;
    inline constexpr qint64 kMaxVideoPixelCount = 36'000'000;
    inline constexpr double kMaxVideoFrameRate = 240.0;
    inline constexpr double kMaxVideoDurationSeconds = 24.0 * 60.0 * 60.0;
    inline constexpr qint64 kMaxVideoFrameCount = 3'000'000;

    struct FfmpegTools
    {
        QString ffmpegPath;
        QString ffprobePath;
        bool bundled = false;
        QString versionLine;
    };

    std::optional<FfmpegTools> locateFfmpegTools(QString *error = nullptr);

    bool isSupportedVideo(const std::filesystem::path &path);

    enum class VideoQuality
    {
        HighQuality,
        Balanced,
        SpaceSaver,
    };

    enum class VideoCodec
    {
        H264,
        Hevc,
    };

    int crfForQuality(VideoQuality quality);

    struct AudioStreamInfo
    {
        QString codec;
        // ISO 639 code the source declares, empty when it declares none. Container metadata is
        // stripped wholesale for privacy, so this one field is re-applied explicitly, the same
        // way the image path allowlists individual EXIF tags.
        QString language;
    };

    struct VideoInfo
    {
        int width = 0;
        int height = 0;
        int rotation = 0;
        int fpsNum = 0;
        int fpsDen = 1;
        double durationSeconds = 0.0;
        // Presentation time of the first video frame. Frame i of the normalized timeline is
        // at startTimeSeconds + i * fpsDen / fpsNum, and every stage must use that mapping.
        double startTimeSeconds = 0.0;
        int sarNum = 1;
        int sarDen = 1;
        qint64 estimatedFrameCount = 0;
        bool isVfr = false;
        QString videoCodec;
        // Every audio stream, in stream order. The output keeps all of them, so this has to
        // describe the whole source rather than just the first track.
        std::vector<AudioStreamInfo> audioStreams;
        QString pixelFormat;
        QString colorTransfer;

        [[nodiscard]] int displayWidth() const;
        [[nodiscard]] int displayHeight() const;
        [[nodiscard]] double fps() const;
    };

    std::optional<VideoInfo> probeVideo(
        const FfmpegTools &tools, const QString &path, QString *error = nullptr);

    QString videoUnsupportedReason(const VideoInfo &info);

    class VideoFrameReader
    {
    public:
        VideoFrameReader();
        ~VideoFrameReader();

        VideoFrameReader(const VideoFrameReader &) = delete;
        VideoFrameReader &operator=(const VideoFrameReader &) = delete;

        bool open(const FfmpegTools &tools,
            const QString &path,
            const VideoInfo &info,
            int decodeLongEdge = 0);
        bool readFrame(cv::Mat &frame, const std::function<bool()> &continueGuard = {});
        void close();

        [[nodiscard]] int frameWidth() const;
        [[nodiscard]] int frameHeight() const;

        [[nodiscard]] bool atEnd() const;
        [[nodiscard]] QString errorString() const;

    private:
        std::unique_ptr<QProcess> process_;
        std::unique_ptr<QLocalServer> server_;
        std::unique_ptr<QLocalSocket> socket_;
        int frameWidth_ = 0;
        int frameHeight_ = 0;
        bool atEnd_ = false;
        QString error_;
    };

    class VideoFrameWriter
    {
    public:
        VideoFrameWriter();
        ~VideoFrameWriter();

        VideoFrameWriter(const VideoFrameWriter &) = delete;
        VideoFrameWriter &operator=(const VideoFrameWriter &) = delete;

        bool open(const FfmpegTools &tools,
            const QString &destination,
            const QString &audioSource,
            const VideoInfo &info,
            int crf,
            bool hardwareEncoder = true,
            VideoCodec codec = VideoCodec::H264,
            const QString &outputRoot = {},
            const QString &relativeDestination = {});
        bool writeFrame(const cv::Mat &frame, const std::function<bool()> &continueGuard = {});
        bool finish(const std::function<bool()> &publishGuard = {});
        void abort();

        [[nodiscard]] QString errorString() const;
        [[nodiscard]] QString encoderName() const;

        // True once `open` has picked a hardware encoder. A caller that wants to retry in
        // software after a failure needs this to know whether a retry can change anything.
        [[nodiscard]] bool usedHardwareEncoder() const;

        // True when the encoder process itself failed. Publication refusals and source-identity
        // failures leave this false, because re-encoding cannot change their outcome.
        [[nodiscard]] bool encoderFailed() const;

    private:
        void releaseStaging();

        void noteEncoderFailure(const QString &message);

        std::unique_ptr<QProcess> process_;
        std::unique_ptr<StageDirectory> stagingDirectory_;
        QString tempPath_;
        QString destinationPath_;
        QString outputRootPath_;
        QString relativeDestinationPath_;
        int frameWidth_ = 0;
        int frameHeight_ = 0;
        QString error_;
        QString encoderName_;
        bool usedHardwareEncoder_ = false;
        bool encoderFailed_ = false;
    };
}
