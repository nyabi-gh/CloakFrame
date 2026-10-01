#include "cloakframe/VideoReviewTypes.hpp"

#include "cloakframe/VideoIo.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>

namespace cloakframe
{
    namespace
    {
        bool validRect(const QRectF &rect)
        {
            return std::isfinite(rect.x()) && std::isfinite(rect.y()) && std::isfinite(rect.width())
                   && std::isfinite(rect.height()) && rect.width() > 0.0 && rect.height() > 0.0;
        }

        QRectF interpolateRect(const QRectF &first, const QRectF &second, const double amount)
        {
            return {
                first.x() + (second.x() - first.x()) * amount,
                first.y() + (second.y() - first.y()) * amount,
                first.width() + (second.width() - first.width()) * amount,
                first.height() + (second.height() - first.height()) * amount,
            };
        }

        bool validManualTrack(const VideoReviewManualTrack &track)
        {
            if (track.startFrame < 0 || track.endFrame < track.startFrame
                || track.keyframes.isEmpty())
            {
                return false;
            }
            int previousFrame = -1;
            for (const auto &keyframe : track.keyframes)
            {
                if (keyframe.frame <= previousFrame || keyframe.frame < track.startFrame
                    || keyframe.frame > track.endFrame || !validRect(keyframe.rect))
                {
                    return false;
                }
                previousFrame = keyframe.frame;
            }
            return true;
        }

        QRectF rectAtFrame(const VideoReviewManualTrack &track, const int frame)
        {
            const auto next = std::lower_bound(track.keyframes.cbegin(),
                track.keyframes.cend(),
                frame,
                [](const VideoReviewBox &box, const int value)
                {
                    return box.frame < value;
                });
            if (next == track.keyframes.cbegin())
            {
                return next->rect;
            }
            if (next == track.keyframes.cend())
            {
                return track.keyframes.back().rect;
            }
            if (next->frame == frame)
            {
                return next->rect;
            }

            const auto previous = std::prev(next);
            const double amount = static_cast<double>(frame - previous->frame)
                                  / static_cast<double>(next->frame - previous->frame);
            return interpolateRect(previous->rect, next->rect, amount);
        }
    }

    std::optional<QRectF> manualTrackRectAtFrame(
        const VideoReviewManualTrack &track, const int frame)
    {
        if (!validManualTrack(track) || frame < track.startFrame || frame > track.endFrame)
        {
            return std::nullopt;
        }

        return rectAtFrame(track, frame);
    }

    std::optional<Track> materializeManualVideoTrack(const VideoReviewManualTrack &track,
        const int frameCount,
        const QSize &frameSize,
        const int assignedId)
    {
        if (!validManualTrack(track) || frameCount <= 0 || !frameSize.isValid()
            || track.endFrame >= frameCount)
        {
            return std::nullopt;
        }

        Track result;
        result.id = assignedId;
        result.boxes.reserve(static_cast<std::size_t>(track.endFrame)
                             - static_cast<std::size_t>(track.startFrame) + 1);
        const QRectF frameBounds(QPointF(0.0, 0.0), QSizeF(frameSize));
        for (int frame = track.startFrame; frame <= track.endFrame; ++frame)
        {
            const QRectF interpolated = rectAtFrame(track, frame);
            const QRectF clipped = interpolated.normalized().intersected(frameBounds);
            if (!validRect(clipped))
            {
                return std::nullopt;
            }
            const auto keyframe = std::lower_bound(track.keyframes.cbegin(),
                track.keyframes.cend(),
                frame,
                [](const VideoReviewBox &box, const int value)
                {
                    return box.frame < value;
                });
            const bool exactKeyframe =
                keyframe != track.keyframes.cend() && keyframe->frame == frame;
            result.boxes.push_back({
                frame,
                cv::Rect2f(static_cast<float>(clipped.x()),
                    static_cast<float>(clipped.y()),
                    static_cast<float>(clipped.width()),
                    static_cast<float>(clipped.height())),
                1.0F,
                !exactKeyframe,
            });
        }
        return result;
    }

    QStringList videoPreviewFrameArguments(const VideoReviewRequest &request, const int frame)
    {
        if (request.ffmpegPath.isEmpty() || request.sourcePath.isEmpty() || request.fps <= 0.0
            || request.fpsNum <= 0 || request.fpsDen <= 0 || frame < 0
            || !request.frameSize.isValid())
        {
            return {};
        }

        int previewWidth = request.frameSize.width();
        int previewHeight = request.frameSize.height();
        const int longEdge = std::max(previewWidth, previewHeight);
        if (longEdge > 960)
        {
            const double scale = 960.0 / static_cast<double>(longEdge);
            previewWidth = std::max(2, static_cast<int>(std::lround(previewWidth * scale)));
            previewHeight = std::max(2, static_cast<int>(std::lround(previewHeight * scale)));
        }
        previewWidth += previewWidth % 2;
        previewHeight += previewHeight % 2;

        const int marginFrames = std::clamp(static_cast<int>(std::ceil(request.fps * 3.0)), 1, 240);
        const int seekFrame = std::max(0, frame - marginFrames);
        const double secondsPerFrame =
            static_cast<double>(request.fpsDen) / static_cast<double>(request.fpsNum);

        // Select on the source timeline rather than on a post-seek frame counter: -ss only
        // lands near the requested time, so counting frames from it picks the wrong one.
        const double frameSeconds =
            request.startTimeSeconds + static_cast<double>(frame) * secondsPerFrame;
        const double tolerance = secondsPerFrame / 2.0;
        const QString filter =
            QString("fps=fps=%1/%2:start_time=%3,select=between(t\\,%4\\,%5),scale=%6:%7:"
                    "flags=area")
                .arg(request.fpsNum)
                .arg(request.fpsDen)
                .arg(QString::number(request.startTimeSeconds, 'f', 6),
                    QString::number(frameSeconds - tolerance, 'f', 6),
                    QString::number(frameSeconds + tolerance, 'f', 6))
                .arg(previewWidth)
                .arg(previewHeight);

        QStringList arguments{"-v", "error", "-nostdin", "-copyts"};
        if (seekFrame > 0)
        {
            const double seekSeconds =
                request.startTimeSeconds + static_cast<double>(seekFrame) * secondsPerFrame;
            arguments << "-ss" << QString::number(seekSeconds, 'f', 6);
        }
        arguments << ffmpegFileInput(request.sourcePath) << "-map" << "0:v:0"
                  << "-vf" << filter << "-frames:v" << "1"
                  << "-f" << "image2pipe" << "-c:v" << "png" << "-";
        return arguments;
    }
}
