#pragma once

// Included at the top of every 1.12.1 test source by transform.py. The transformed sources call
// cfrec:: wrappers instead of the core API, so each test's real inputs and outputs are written as
// fixtures while the original assertions still run.

#include "cloakframe/DetectionGeometry.hpp"
#include "cloakframe/ImageIo.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/PathUtil.hpp"
#include "cloakframe/SceneCut.hpp"
#include "cloakframe/Tracking.hpp"
#include "cloakframe/UpdateSignature.hpp"
#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/VideoReviewTypes.hpp"

#include <QByteArray>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>

#include <opencv2/core.hpp>
#include <opencv2/imgcodecs.hpp>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <map>
#include <mutex>
#include <string>
#include <vector>

namespace cfrec
{
    struct AssertRecord
    {
        int line = 0;
        std::string expr;
        int passed = 0;
        int failed = 0;
    };

    struct TestRecord
    {
        QJsonArray calls;
        std::map<int, AssertRecord> asserts;
        int imageCount = 0;
        int fileCount = 0;
        int skippedCalls = 0;
        std::uintmax_t savedBytes = 0;
    };

    struct State
    {
        std::mutex mutex;
        std::string suite;
        std::string current;
        std::vector<std::string> order;
        std::map<std::string, TestRecord> tests;
        std::filesystem::path out;
        bool enabled = false;
        bool registered = false;
    };

    inline State &state()
    {
        static State s;
        return s;
    }

    constexpr int kMaxCallsPerTest = 64;
    constexpr int kMaxImagesPerTest = 24;
    constexpr qsizetype kMaxCallJsonBytes = 256 * 1024;
    constexpr std::uintmax_t kMaxSavedBytesPerTest = 1024 * 1024;

    inline void addSaved(std::uintmax_t bytes)
    {
        auto &s = state();
        std::lock_guard lock(s.mutex);
        s.tests[s.current].savedBytes += bytes;
    }
    constexpr std::uintmax_t kMaxCopiedFileBytes = 512 * 1024;

    inline std::filesystem::path testDir()
    {
        auto &s = state();
        return s.out / s.suite / s.current;
    }

    inline void flush()
    {
        auto &s = state();
        std::lock_guard lock(s.mutex);
        if (!s.enabled)
        {
            return;
        }
        std::filesystem::create_directories(s.out / s.suite);
        for (const auto &name : s.order)
        {
            const auto &record = s.tests[name];
            QJsonObject doc;
            doc["suite"] = QString::fromStdString(s.suite);
            doc["test"] = QString::fromStdString(name);
            doc["source"] = QStringLiteral("tests/%1.cpp @ v1.12.1").arg(QString::fromStdString(s.suite));
            QJsonArray asserts;
            for (const auto &[line, a] : record.asserts)
            {
                asserts.append(QJsonObject{{"line", line},
                    {"expr", QString::fromStdString(a.expr)},
                    {"passed", a.passed},
                    {"failed", a.failed}});
            }
            doc["asserts_cpp"] = asserts;
            doc["calls"] = record.calls;
            if (record.skippedCalls > 0)
            {
                doc["calls_not_recorded"] = record.skippedCalls;
            }
            QFile file(QString::fromStdString((s.out / s.suite / (name + ".json")).string()));
            if (file.open(QIODevice::WriteOnly | QIODevice::Truncate))
            {
                file.write(QJsonDocument(doc).toJson(QJsonDocument::Indented));
            }
        }
    }

    inline void begin(const char *test)
    {
        auto &s = state();
        std::lock_guard lock(s.mutex);
        if (!s.registered)
        {
            s.registered = true;
            const char *out = std::getenv("CFREC_OUT");
            s.enabled = out != nullptr && *out != '\0';
            if (s.enabled)
            {
                s.out = out;
                s.suite = getprogname();
                const std::string legacyPrefix = "cloakframe_";
                if (s.suite.starts_with(legacyPrefix))
                {
                    s.suite = s.suite.substr(legacyPrefix.size());
                }
                std::atexit(flush);
            }
        }
        s.current = test;
        s.order.push_back(test);
    }

    inline bool check(const char *expr, bool value, int line)
    {
        auto &s = state();
        {
            std::lock_guard lock(s.mutex);
            if (!s.current.empty())
            {
                auto &a = s.tests[s.current].asserts[line];
                a.line = line;
                a.expr = expr;
                ++(value ? a.passed : a.failed);
            }
        }
        if (!value)
        {
            std::fprintf(stderr, "assertion failed at line %d: %s\n", line, expr);
            flush();
            std::abort();
        }
        return value;
    }

    // Returns false when the per-test call budget is spent, so loops that call the API thousands
    // of times keep the fixture readable.
    inline bool record(QJsonObject call)
    {
        auto &s = state();
        std::lock_guard lock(s.mutex);
        if (!s.enabled || s.current.empty())
        {
            return false;
        }
        auto &test = s.tests[s.current];
        if (test.calls.size() >= kMaxCallsPerTest)
        {
            ++test.skippedCalls;
            return false;
        }
        if (QJsonDocument(call).toJson(QJsonDocument::Compact).size() > kMaxCallJsonBytes)
        {
            // Stress inputs (safety-limit tests) would make megabyte fixtures; keep their shape.
            for (const auto &key : call.keys())
            {
                const auto value = call[key];
                if (value.isArray() && value.toArray().size() > 16)
                {
                    call[key] = QJsonObject{{"omitted_large_array", true}, {"length", value.toArray().size()}};
                }
            }
        }
        test.calls.append(call);
        return true;
    }

    inline bool budgetLeft()
    {
        auto &s = state();
        std::lock_guard lock(s.mutex);
        if (!s.enabled || s.current.empty())
        {
            return false;
        }
        const auto &test = s.tests[s.current];
        return test.calls.size() < kMaxCallsPerTest && test.imageCount + 3 <= kMaxImagesPerTest
               && test.savedBytes < kMaxSavedBytesPerTest;
    }

    inline QString saveImage(const cv::Mat &image, const char *role)
    {
        if (image.empty())
        {
            return {};
        }
        auto &s = state();
        int index = 0;
        {
            std::lock_guard lock(s.mutex);
            index = ++s.tests[s.current].imageCount;
        }
        const auto dir = testDir();
        std::filesystem::create_directories(dir);
        const std::string name = std::to_string(index) + "-" + role + ".png";
        cv::imwrite((dir / name).string(), image);
        std::error_code ec;
        addSaved(std::filesystem::file_size(dir / name, ec));
        return QString::fromStdString(s.current + "/" + name);
    }

    inline QJsonValue saveFile(const std::filesystem::path &path, const char *role)
    {
        std::error_code ec;
        if (!std::filesystem::is_regular_file(path, ec))
        {
            return QJsonObject{{"exists", false}, {"name", QString::fromStdString(path.filename().string())}};
        }
        const auto size = std::filesystem::file_size(path, ec);
        QFile file(QString::fromStdString(path.string()));
        QByteArray bytes;
        if (file.open(QIODevice::ReadOnly))
        {
            bytes = file.readAll();
        }
        QJsonObject value{{"name", QString::fromStdString(path.filename().string())},
            {"size", static_cast<qint64>(size)},
            {"sha256", QString::fromLatin1(QCryptographicHash::hash(bytes, QCryptographicHash::Sha256).toHex())}};
        auto &s = state();
        bool fits = false;
        int index = 0;
        {
            std::lock_guard lock(s.mutex);
            auto &test = s.tests[s.current];
            fits = size <= kMaxCopiedFileBytes && test.savedBytes + size <= kMaxSavedBytesPerTest;
            if (fits)
            {
                index = ++test.fileCount;
                test.savedBytes += size;
            }
        }
        if (fits)
        {
            const auto dir = testDir();
            std::filesystem::create_directories(dir);
            const std::string name = "file" + std::to_string(index) + "-" + role + path.extension().string();
            std::filesystem::copy_file(path, dir / name, std::filesystem::copy_options::overwrite_existing, ec);
            value["copy"] = QString::fromStdString(s.current + "/" + name);
        }
        return value;
    }

    inline QJsonValue num(double v)
    {
        if (std::isnan(v))
        {
            return QStringLiteral("NaN");
        }
        if (std::isinf(v))
        {
            return v > 0 ? QStringLiteral("Infinity") : QStringLiteral("-Infinity");
        }
        return v;
    }

    inline QJsonArray rect(const cv::Rect2f &r)
    {
        return {num(r.x), num(r.y), num(r.width), num(r.height)};
    }

    inline QJsonArray rect(const QRectF &r)
    {
        return {r.x(), r.y(), r.width(), r.height()};
    }

    inline QJsonObject toJson(const cloakframe::FaceDetection &d)
    {
        return {{"box", rect(d.box)}, {"score", num(d.score)}, {"roll", num(d.rollRadians)}, {"has_pose", d.hasPose}};
    }

    inline QJsonArray toJson(const cloakframe::FaceDetections &ds)
    {
        QJsonArray a;
        for (const auto &d : ds)
        {
            a.append(toJson(d));
        }
        return a;
    }

    inline QJsonArray toJson(const std::vector<cloakframe::FaceDetections> &frames)
    {
        QJsonArray a;
        for (const auto &f : frames)
        {
            a.append(toJson(f));
        }
        return a;
    }

    inline QJsonObject toJson(const cloakframe::Track &t)
    {
        QJsonArray boxes;
        for (const auto &b : t.boxes)
        {
            boxes.append(QJsonObject{{"frame", b.frame},
                {"box", rect(b.box)},
                {"score", num(b.score)},
                {"interpolated", b.interpolated},
                {"roll", num(b.rollRadians)},
                {"has_pose", b.hasPose}});
        }
        return {{"id", t.id}, {"low_confidence", t.lowConfidence}, {"boxes", boxes}};
    }

    inline QJsonArray toJson(const std::vector<cloakframe::Track> &ts)
    {
        QJsonArray a;
        for (const auto &t : ts)
        {
            a.append(toJson(t));
        }
        return a;
    }

    inline QJsonArray toJson(const cloakframe::SceneCuts &cuts)
    {
        QJsonArray a;
        for (const int f : cuts.frames())
        {
            a.append(f);
        }
        return a;
    }

    inline QJsonArray toJson(const std::vector<cloakframe::UncoveredSpan> &spans)
    {
        QJsonArray a;
        for (const auto &s : spans)
        {
            a.append(QJsonArray{s.firstFrame, s.lastFrame});
        }
        return a;
    }

    inline QJsonObject toJson(const cloakframe::TrackerConfig &c)
    {
        return {{"high_score_threshold", num(c.highScoreThreshold)},
            {"low_score_threshold", num(c.lowScoreThreshold)},
            {"iou_threshold", num(c.iouThreshold)},
            {"max_frames_lost", c.maxFramesLost},
            {"max_frames_since_high_score", c.maxFramesSinceHighScore},
            {"max_frames_since_high_score_moving", c.maxFramesSinceHighScoreMoving},
            {"coast_motion_threshold", num(c.coastMotionThreshold)},
            {"velocity_blend", num(c.velocityBlend)}};
    }

    inline QJsonObject toJson(const cloakframe::TrackPostProcessConfig &c)
    {
        return {{"max_interpolation_gap", c.maxInterpolationGap},
            {"smoothing_radius", c.smoothingRadius},
            {"extension_frames", c.extensionFrames},
            {"max_cut_boundary_gap", c.maxCutBoundaryGap},
            {"strong_score_threshold", num(c.strongScoreThreshold)},
            {"min_strong_detections", c.minStrongDetections},
            {"short_track_min_strong", c.shortTrackMinStrong},
            {"short_track_strong_ratio", num(c.shortTrackStrongRatio)},
            {"retain_tracks_without_strong_detection", c.retainTracksWithoutStrongDetection}};
    }

    inline QJsonObject toJson(const cloakframe::TrackingWindows &w)
    {
        return {{"max_lost_seconds", w.maxLostSeconds},
            {"max_since_high_score_seconds", w.maxSinceHighScoreSeconds},
            {"max_since_high_score_moving_seconds", w.maxSinceHighScoreMovingSeconds},
            {"max_interpolation_seconds", w.maxInterpolationSeconds},
            {"extension_seconds", w.extensionSeconds},
            {"max_cut_boundary_seconds", w.maxCutBoundarySeconds}};
    }

    inline QJsonObject toJson(const cloakframe::TrackCoverageReport &r)
    {
        return {{"uncovered_frames", r.uncoveredFrames},
            {"dropped_tracks", r.droppedTracks},
            {"uncovered_spans", toJson(r.uncoveredSpans)}};
    }

    inline QJsonObject toJson(const cloakframe::VideoReviewManualTrack &t)
    {
        QJsonArray keys;
        for (const auto &k : t.keyframes)
        {
            keys.append(QJsonObject{{"frame", k.frame}, {"rect", rect(k.rect)}, {"interpolated", k.interpolated}});
        }
        return {{"id", t.id}, {"start_frame", t.startFrame}, {"end_frame", t.endFrame}, {"keyframes", keys}};
    }

    inline QJsonObject toJson(const cloakframe::VideoInfo &i)
    {
        QJsonArray audio;
        for (const auto &a : i.audioStreams)
        {
            audio.append(QJsonObject{{"codec", a.codec}, {"language", a.language}});
        }
        return {{"width", i.width},
            {"height", i.height},
            {"rotation", i.rotation},
            {"fps_num", i.fpsNum},
            {"fps_den", i.fpsDen},
            {"duration_seconds", i.durationSeconds},
            {"start_time_seconds", i.startTimeSeconds},
            {"sar_num", i.sarNum},
            {"sar_den", i.sarDen},
            {"estimated_frame_count", i.estimatedFrameCount},
            {"is_vfr", i.isVfr},
            {"video_codec", i.videoCodec},
            {"audio_streams", audio},
            {"pixel_format", i.pixelFormat},
            {"color_transfer", i.colorTransfer}};
    }

    inline const char *methodName(cloakframe::AnonymizationMethod m)
    {
        switch (m)
        {
        case cloakframe::AnonymizationMethod::Mosaic: return "mosaic";
        case cloakframe::AnonymizationMethod::Blur: return "blur";
        case cloakframe::AnonymizationMethod::Fill: return "fill";
        case cloakframe::AnonymizationMethod::CustomImage: return "custom_image";
        }
        return "?";
    }

    inline QJsonObject matInfo(const cv::Mat &m)
    {
        return {{"rows", m.rows}, {"cols", m.cols}, {"channels", m.channels()}, {"depth_bits", static_cast<int>(m.elemSize1() * 8)}};
    }

    inline QJsonArray smallMat(const cv::Mat &m)
    {
        QJsonArray rows;
        for (int r = 0; r < m.rows; ++r)
        {
            QJsonArray row;
            for (int c = 0; c < m.cols; ++c)
            {
                row.append(m.at<uchar>(r, c));
            }
            rows.append(row);
        }
        return rows;
    }


    inline std::vector<cloakframe::Track> buildTracks(const std::vector<cloakframe::FaceDetections> &frames,
        const cloakframe::TrackerConfig &config = {},
        const cloakframe::SceneCuts &cuts = {},
        const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "buildTracks"},
            {"frames", toJson(frames)},
            {"config", toJson(config)},
            {"scene_cuts", toJson(cuts)},
            {"has_continue_guard", static_cast<bool>(guard)}};
        try
        {
            auto out = cloakframe::buildTracks(frames, config, cuts, guard);
            call["tracks"] = toJson(out);
            record(call);
            return out;
        }
        catch (const std::exception &e)
        {
            call["throws"] = QString::fromUtf8(e.what());
            record(call);
            throw;
        }
    }

    inline std::vector<cloakframe::Track> buildBidirectionalTracks(
        const std::vector<cloakframe::FaceDetections> &frames,
        const cloakframe::TrackerConfig &config = {},
        float mergeIouThreshold = 0.5F,
        const cloakframe::SceneCuts &cuts = {},
        const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "buildBidirectionalTracks"},
            {"frames", toJson(frames)},
            {"config", toJson(config)},
            {"merge_iou_threshold", num(mergeIouThreshold)},
            {"scene_cuts", toJson(cuts)},
            {"has_continue_guard", static_cast<bool>(guard)}};
        try
        {
            auto out = cloakframe::buildBidirectionalTracks(frames, config, mergeIouThreshold, cuts, guard);
            call["tracks"] = toJson(out);
            record(call);
            return out;
        }
        catch (const std::exception &e)
        {
            call["throws"] = QString::fromUtf8(e.what());
            record(call);
            throw;
        }
    }

    inline std::vector<cloakframe::UncoveredSpan> interpolateGaps(cloakframe::Track &track,
        int maxGap,
        const cloakframe::SceneCuts &cuts = {},
        const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "interpolateGaps"},
            {"track_before", toJson(track)},
            {"max_gap", maxGap},
            {"scene_cuts", toJson(cuts)},
            {"has_continue_guard", static_cast<bool>(guard)}};
        auto out = cloakframe::interpolateGaps(track, maxGap, cuts, guard);
        call["track_after"] = toJson(track);
        call["uncovered_spans"] = toJson(out);
        record(call);
        return out;
    }

    inline void smoothTrack(cloakframe::Track &track, int radius, const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "smoothTrack"}, {"track_before", toJson(track)}, {"radius", radius}};
        cloakframe::smoothTrack(track, radius, guard);
        call["track_after"] = toJson(track);
        record(call);
    }

    inline void extendTrackEnds(cloakframe::Track &track,
        int frames,
        int frameCount,
        const cloakframe::SceneCuts &cuts = {},
        const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "extendTrackEnds"},
            {"track_before", toJson(track)},
            {"frames", frames},
            {"frame_count", frameCount},
            {"scene_cuts", toJson(cuts)}};
        cloakframe::extendTrackEnds(track, frames, frameCount, cuts, guard);
        call["track_after"] = toJson(track);
        record(call);
    }

    inline cloakframe::TrackCoverageReport postProcessTracks(std::vector<cloakframe::Track> &tracks,
        const cloakframe::TrackPostProcessConfig &config,
        int frameCount,
        const cloakframe::SceneCuts &cuts = {},
        const cloakframe::TrackingContinueGuard &guard = {})
    {
        QJsonObject call{{"fn", "postProcessTracks"},
            {"tracks_before", toJson(tracks)},
            {"config", toJson(config)},
            {"frame_count", frameCount},
            {"scene_cuts", toJson(cuts)},
            {"has_continue_guard", static_cast<bool>(guard)}};
        try
        {
            auto out = cloakframe::postProcessTracks(tracks, config, frameCount, cuts, guard);
            call["tracks_after"] = toJson(tracks);
            call["report"] = toJson(out);
            record(call);
            return out;
        }
        catch (const std::exception &e)
        {
            call["throws"] = QString::fromUtf8(e.what());
            record(call);
            throw;
        }
    }

    inline void applyTrackingWindows(const cloakframe::TrackingWindows &windows,
        double fps,
        cloakframe::TrackerConfig &tracker,
        cloakframe::TrackPostProcessConfig &postProcess)
    {
        QJsonObject call{{"fn", "applyTrackingWindows"}, {"windows", toJson(windows)}, {"fps", fps}};
        cloakframe::applyTrackingWindows(windows, fps, tracker, postProcess);
        call["tracker"] = toJson(tracker);
        call["post_process"] = toJson(postProcess);
        record(call);
    }

    inline std::optional<QRectF> manualTrackRectAtFrame(const cloakframe::VideoReviewManualTrack &track, int frame)
    {
        auto out = cloakframe::manualTrackRectAtFrame(track, frame);
        record(QJsonObject{{"fn", "manualTrackRectAtFrame"},
            {"track", toJson(track)},
            {"frame", frame},
            {"rect", out ? QJsonValue(rect(*out)) : QJsonValue()}});
        return out;
    }

    inline std::optional<cloakframe::Track> materializeManualVideoTrack(
        const cloakframe::VideoReviewManualTrack &track, int frameCount, const QSize &frameSize, int assignedId)
    {
        auto out = cloakframe::materializeManualVideoTrack(track, frameCount, frameSize, assignedId);
        record(QJsonObject{{"fn", "materializeManualVideoTrack"},
            {"track", toJson(track)},
            {"frame_count", frameCount},
            {"frame_size", QJsonArray{frameSize.width(), frameSize.height()}},
            {"assigned_id", assignedId},
            {"result", out ? QJsonValue(toJson(*out)) : QJsonValue()}});
        return out;
    }

    // Scene-cut fixtures describe each frame by its content so the input stays small and
    // language neutral: the 1.12.1 tests only push uniform frames and full-range gradients.
    inline QJsonObject describeFrame(const cv::Mat &frame)
    {
        QJsonObject d{{"width", frame.cols}, {"height", frame.rows}, {"channels", frame.channels()}};
        double minValue = 0;
        double maxValue = 0;
        cv::minMaxLoc(frame.reshape(1), &minValue, &maxValue);
        if (minValue == maxValue)
        {
            d["kind"] = "solid";
            d["value"] = minValue;
            return d;
        }
        if (frame.type() == CV_8UC1)
        {
            for (const bool horizontal : {true, false})
            {
                bool matches = true;
                for (int y = 0; y < frame.rows && matches; ++y)
                {
                    for (int x = 0; x < frame.cols; ++x)
                    {
                        const int expected = horizontal ? x * 255 / (frame.cols - 1) : y * 255 / (frame.rows - 1);
                        if (frame.at<uchar>(y, x) != expected)
                        {
                            matches = false;
                            break;
                        }
                    }
                }
                if (matches)
                {
                    d["kind"] = horizontal ? "gradient_x" : "gradient_y";
                    d["formula"] = horizontal ? "x * 255 / (width - 1)" : "y * 255 / (height - 1)";
                    return d;
                }
            }
        }
        d["kind"] = "image";
        d["png"] = saveImage(frame, "frame");
        return d;
    }

    class SceneCutDetector
    {
    public:
        void push(const cv::Mat &frame)
        {
            const auto d = describeFrame(frame);
            if (!frames_.isEmpty() && frames_.last().toObject()["frame"].toObject() == d)
            {
                auto run = frames_.last().toObject();
                run["repeat"] = run["repeat"].toInt() + 1;
                frames_[frames_.size() - 1] = run;
            }
            else
            {
                frames_.append(QJsonObject{{"frame", d}, {"repeat", 1}});
            }
            inner_.push(frame);
        }

        [[nodiscard]] cloakframe::SceneCuts finish()
        {
            auto cuts = inner_.finish();
            record(QJsonObject{{"fn", "SceneCutDetector"}, {"pushed", frames_}, {"cuts", toJson(cuts)}});
            return cuts;
        }

    private:
        cloakframe::SceneCutDetector inner_;
        QJsonArray frames_;
    };


    inline void applyAnonymization(cv::Mat &image,
        const cloakframe::FaceDetections &detections,
        cloakframe::AnonymizationMethod method,
        int blockSize,
        float paddingRatio,
        cloakframe::MaskShape shape = cloakframe::MaskShape::Rectangle,
        bool softEdges = false,
        const cv::Mat &customImage = {})
    {
        const bool keep = budgetLeft();
        QJsonObject call;
        if (keep)
        {
            call = {{"fn", "applyAnonymization"},
                {"image", matInfo(image)},
                {"input_png", saveImage(image, "input")},
                {"detections", toJson(detections)},
                {"method", methodName(method)},
                {"block_size", blockSize},
                {"padding_ratio", num(paddingRatio)},
                {"shape", shape == cloakframe::MaskShape::Ellipse ? "ellipse" : "rectangle"},
                {"soft_edges", softEdges}};
            if (!customImage.empty())
            {
                call["custom_image"] = matInfo(customImage);
                call["custom_image_png"] = saveImage(customImage, "custom");
            }
        }
        cloakframe::applyAnonymization(image, detections, method, blockSize, paddingRatio, shape, softEdges, customImage);
        if (keep)
        {
            call["output_png"] = saveImage(image, "output");
        }
        record(call);
    }

    inline void applyMosaic(cv::Mat &image, const cloakframe::FaceDetections &detections, int blockSize, float paddingRatio)
    {
        QJsonObject call{{"fn", "applyMosaic"},
            {"image", matInfo(image)},
            {"input_png", saveImage(image, "input")},
            {"detections", toJson(detections)},
            {"block_size", blockSize},
            {"padding_ratio", num(paddingRatio)}};
        cloakframe::applyMosaic(image, detections, blockSize, paddingRatio);
        call["output_png"] = saveImage(image, "output");
        record(call);
    }

    inline void applyOrientation(cv::Mat &image, int orientation)
    {
        QJsonObject call{{"fn", "applyOrientation"}, {"orientation", orientation}, {"image", matInfo(image)}};
        const bool small = image.type() == CV_8UC1 && image.total() <= 64;
        if (small)
        {
            call["input"] = smallMat(image);
        }
        cloakframe::applyOrientation(image, orientation);
        call["output_image"] = matInfo(image);
        if (small)
        {
            call["output"] = smallMat(image);
        }
        record(call);
    }

    inline cloakframe::FaceDetections nonMaxSuppression(cloakframe::FaceDetections detections, float threshold)
    {
        QJsonObject call{{"fn", "nonMaxSuppression"}, {"detections", toJson(detections)}, {"threshold", num(threshold)}};
        auto out = cloakframe::nonMaxSuppression(std::move(detections), threshold);
        call["kept"] = toJson(out);
        record(call);
        return out;
    }

    inline float intersectionOverUnion(const cv::Rect2f &a, const cv::Rect2f &b)
    {
        const float out = cloakframe::intersectionOverUnion(a, b);
        record(QJsonObject{{"fn", "intersectionOverUnion"}, {"a", rect(a)}, {"b", rect(b)}, {"iou", num(out)}});
        return out;
    }

    inline std::vector<int> encodeParamsForExtension(const std::string &ext)
    {
        auto out = cloakframe::encodeParamsForExtension(ext);
        QJsonArray params;
        for (const int v : out)
        {
            params.append(v);
        }
        record(QJsonObject{{"fn", "encodeParamsForExtension"},
            {"extension", QString::fromStdString(ext)},
            {"opencv_params", params}});
        return out;
    }

    inline int readExifOrientation(const std::filesystem::path &source)
    {
        const int out = cloakframe::readExifOrientation(source);
        record(QJsonObject{{"fn", "readExifOrientation"}, {"file", saveFile(source, "input")}, {"orientation", out}});
        return out;
    }

    inline std::optional<cloakframe::ImageFormat> sniffImageFormat(const std::filesystem::path &path)
    {
        auto out = cloakframe::sniffImageFormat(path);
        record(QJsonObject{{"fn", "sniffImageFormat"},
            {"file", saveFile(path, "input")},
            {"format", out ? QJsonValue(static_cast<int>(*out)) : QJsonValue()}});
        return out;
    }

    inline std::size_t imageFrameCount(const std::filesystem::path &source)
    {
        const auto out = cloakframe::imageFrameCount(source);
        record(QJsonObject{{"fn", "imageFrameCount"}, {"file", saveFile(source, "input")}, {"frames", static_cast<qint64>(out)}});
        return out;
    }

    inline bool copyMetadata(const std::filesystem::path &source,
        const std::filesystem::path &destination,
        bool preserve,
        bool preserveLocation = false)
    {
        QJsonObject call{{"fn", "copyMetadata"},
            {"source", saveFile(source, "source")},
            {"destination_before", saveFile(destination, "destination-before")},
            {"preserve", preserve},
            {"preserve_location", preserveLocation}};
        const bool out = cloakframe::copyMetadata(source, destination, preserve, preserveLocation);
        call["result"] = out;
        call["destination_after"] = saveFile(destination, "destination-after");
        record(call);
        return out;
    }


    inline int crfForQuality(cloakframe::VideoQuality quality)
    {
        const int out = cloakframe::crfForQuality(quality);
        record(QJsonObject{{"fn", "crfForQuality"}, {"quality", static_cast<int>(quality)}, {"crf", out}});
        return out;
    }

    inline QString videoUnsupportedReason(const cloakframe::VideoInfo &info)
    {
        const auto out = cloakframe::videoUnsupportedReason(info);
        record(QJsonObject{{"fn", "videoUnsupportedReason"}, {"info", toJson(info)}, {"reason_en", out}});
        return out;
    }

    inline std::optional<cloakframe::VideoInfo> probeVideo(
        const cloakframe::FfmpegTools &tools, const QString &path, QString *error = nullptr)
    {
        auto out = cloakframe::probeVideo(tools, path, error);
        record(QJsonObject{{"fn", "probeVideo"},
            {"file", saveFile(cloakframe::pathFromQString(path), "input")},
            {"info", out ? QJsonValue(toJson(*out)) : QJsonValue()}});
        return out;
    }

    inline bool isSupportedVideo(const std::filesystem::path &path)
    {
        const bool out = cloakframe::isSupportedVideo(path);
        record(QJsonObject{{"fn", "isSupportedVideo"}, {"name", QString::fromStdString(path.filename().string())}, {"result", out}});
        return out;
    }

    inline float videoStrongScoreThreshold(float scoreThreshold)
    {
        const float out = cloakframe::videoStrongScoreThreshold(scoreThreshold);
        record(QJsonObject{{"fn", "videoStrongScoreThreshold"}, {"score_threshold", num(scoreThreshold)}, {"strong", num(out)}});
        return out;
    }

    inline cloakframe::VideoMaskingPlan videoMaskingPlan(int width, int height, unsigned int threads, qint64 memoryBudget = 0)
    {
        const auto out = cloakframe::videoMaskingPlan(width, height, threads, memoryBudget);
        record(QJsonObject{{"fn", "videoMaskingPlan"},
            {"width", width},
            {"height", height},
            {"hardware_threads", static_cast<int>(threads)},
            {"memory_budget", memoryBudget},
            {"frame_bytes", out.frameBytes},
            {"worker_count", out.workerCount},
            {"batch_frames", out.batchFrames},
            {"prefetch_frames", out.prefetchFrames}});
        return out;
    }


    inline bool verifyUpdateSignature(const QByteArray &payload,
        const QString &signatureBase64,
        const QString &publicKeyBase64,
        QString *error = nullptr)
    {
        const bool out = cloakframe::verifyUpdateSignature(payload, signatureBase64, publicKeyBase64, error);
        record(QJsonObject{{"fn", "verifyUpdateSignature"},
            {"payload_base64", QString::fromLatin1(payload.toBase64())},
            {"signature_base64", signatureBase64},
            {"public_key_base64", publicKeyBase64},
            {"result", out}});
        return out;
    }

    inline QJsonObject toJson(const cloakframe::UpdateRelease &r)
    {
        return {{"channel", r.channel}, {"version", r.version}, {"file_name", r.fileName}, {"sha256_hex", r.sha256Hex}};
    }

    inline std::optional<QByteArray> updateSignaturePayload(const cloakframe::UpdateRelease &release, QString *error = nullptr)
    {
        auto out = cloakframe::updateSignaturePayload(release, error);
        record(QJsonObject{{"fn", "updateSignaturePayload"},
            {"release", toJson(release)},
            {"payload_base64", out ? QJsonValue(QString::fromLatin1(out->toBase64())) : QJsonValue()}});
        return out;
    }

    inline cloakframe::UpdateTrust evaluateUpdateTrust(const cloakframe::UpdateRelease &release,
        const QString &signatureBase64,
        const QString &pinnedPublicKeyBase64,
        QString *error = nullptr)
    {
        const auto out = cloakframe::evaluateUpdateTrust(release, signatureBase64, pinnedPublicKeyBase64, error);
        const char *names[] = {"trusted", "unpinned", "rejected"};
        record(QJsonObject{{"fn", "evaluateUpdateTrust"},
            {"release", toJson(release)},
            {"signature_base64", signatureBase64},
            {"pinned_public_key_base64", pinnedPublicKeyBase64},
            {"trust", names[static_cast<int>(out)]}});
        return out;
    }
}

#define CF_ASSERT(...) ::cfrec::check(#__VA_ARGS__, static_cast<bool>(__VA_ARGS__), __LINE__)
