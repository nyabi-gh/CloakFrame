// Reproduces the plan's D/P defects on the 1.12.1 core and records what 1.12.1 actually does.
// Measurements that need external tools (ffprobe, ImageMagick, ICC parsing) are finished by
// analyze.py from the files written here.
//
// Usage: cloakframe_fixture_defects <fixtures dir> <models dir> <work dir>

#include "cloakframe/DetectionGeometry.hpp"
#include "cloakframe/ImageIo.hpp"
#include "cloakframe/ModelStore.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/SceneCut.hpp"
#include "cloakframe/Tracking.hpp"
#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/VideoReviewTypes.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"

#include <exiv2/exiv2.hpp>

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <sys/stat.h>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <mutex>
#include <tuple>
#include <set>
#include <thread>

namespace fs = std::filesystem;

namespace
{
    fs::path g_fixtures;
    fs::path g_models;
    fs::path g_work;

    void writeJson(const fs::path &path, const QJsonObject &object)
    {
        fs::create_directories(path.parent_path());
        QFile file(QString::fromStdString(path.string()));
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        {
            std::fprintf(stderr, "cannot write %s\n", path.c_str());
            std::exit(1);
        }
        file.write(QJsonDocument(object).toJson(QJsonDocument::Indented));
    }

    QString q(const fs::path &p)
    {
        return QString::fromStdString(p.string());
    }

    QString octal(mode_t mode)
    {
        return QString::number(mode & 0777, 8).rightJustified(4, '0');
    }

    QString modeOf(const fs::path &p)
    {
        struct stat st{};
        if (::stat(p.c_str(), &st) != 0)
        {
            return {};
        }
        return octal(st.st_mode);
    }

    QJsonArray rect(const cv::Rect2f &r)
    {
        return {r.x, r.y, r.width, r.height};
    }

    QJsonObject runWorker(cloakframe::ProcessingRequest request)
    {
        QJsonObject result;
        QJsonArray logs;
        QJsonArray files;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::logMessage,
            [&](const QString &message)
            {
                logs.append(message);
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary summary)
            {
                result["summary"] = QJsonObject{{"total", summary.total},
                    {"redacted", summary.redacted},
                    {"copied", summary.copied},
                    {"unredacted", summary.unredacted}};
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome outcome)
            {
                const char *names[] = {"completed", "completed_with_warnings", "cancelled", "failed"};
                result["outcome"] = names[static_cast<int>(outcome)];
            });
        worker.process();
        result["log"] = logs;
        return result;
    }

    void d2Icc()
    {
        const auto dir = g_fixtures / "defects" / "D2";
        const auto out = g_work / "D2" / "out";
        fs::remove_all(g_work / "D2");
        QStringList inputs;
        for (const auto &entry : fs::directory_iterator(dir))
        {
            if (entry.path().filename().string().starts_with("chart-"))
            {
                inputs << q(entry.path());
            }
        }
        inputs.sort();
        cloakframe::ProcessingRequest request;
        request.inputs = inputs;
        request.outputDirectory = q(out);
        request.detectFaces = false;
        request.recursive = false;
        auto run = runWorker(std::move(request));
        run["output_dir"] = q(out);
        writeJson(g_work / "D2" / "worker.json", run);
    }

    void d3Mosaic()
    {
        const cv::Mat source =
            cv::imread((g_fixtures / "defects" / "D3" / "noise-face-600.png").string(), cv::IMREAD_COLOR);
        const cv::Rect box(100, 100, 600, 600);
        QJsonArray runs;
        for (const int block : {4, 8, 14, 20, 40, 60, 100})
        {
            cv::Mat image = source.clone();
            cloakframe::applyMosaic(image, {{cv::Rect2f(box), 1.0F}}, block, 0.0F);
            std::set<std::uint32_t> colors;
            const cv::Mat region = image(box);
            for (int y = 0; y < region.rows; ++y)
            {
                for (int x = 0; x < region.cols; ++x)
                {
                    const auto p = region.at<cv::Vec3b>(y, x);
                    colors.insert((p[0] << 16) | (p[1] << 8) | p[2]);
                }
            }
            // Horizontal cell count along the middle row: the number of colour runs.
            int runsAcross = 1;
            for (int x = 1; x < region.cols; ++x)
            {
                if (region.at<cv::Vec3b>(300, x) != region.at<cv::Vec3b>(300, x - 1))
                {
                    ++runsAcross;
                }
            }
            runs.append(QJsonObject{{"block_size_px", block},
                {"unique_colors_in_box", static_cast<int>(colors.size())},
                {"cells_across_middle_row", runsAcross}});
        }
        writeJson(g_fixtures / "defects" / "D3" / "observed_1121.json",
            QJsonObject{{"input", "noise-face-600.png"},
                {"region", QJsonArray{box.x, box.y, box.width, box.height}},
                {"function", "applyMosaic(image, region, block, padding=0)"},
                {"runs", runs}});
    }

    QJsonObject d4Write(mode_t mask)
    {
        const auto root = g_work / "D4" / ("umask-" + octal(mask).toStdString());
        fs::remove_all(root);
        fs::create_directories(root);
        const mode_t previous = ::umask(mask);
        const cv::Mat image(32, 32, CV_8UC3, cv::Scalar(30, 60, 90));
        const auto result = cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "sub/out.png", image);
        ::umask(previous);
        return {{"umask", octal(mask)},
            {"result", result == cloakframe::ImageWriteResult::Saved ? "saved" : "not_saved"},
            {"file_mode", modeOf(root / "sub" / "out.png")},
            {"subfolder_mode", modeOf(root / "sub")}};
    }

    void d4Permissions()
    {
        writeJson(g_fixtures / "defects" / "D4" / "observed_1121.json",
            QJsonObject{{"platform", "macOS (POSIX path)"},
                {"function", "imwriteUnicodeNoReplaceAtRoot(root, \"sub/out.png\", image)"},
                {"runs", QJsonArray{d4Write(022), d4Write(002), d4Write(077)}}});
    }

    void d5ModelStore()
    {
        QJsonArray runs;
        for (const mode_t mask : {mode_t(022), mode_t(002)})
        {
            const auto root = g_work / "D5" / ("umask-" + octal(mask).toStdString());
            fs::remove_all(root);
            const mode_t previous = ::umask(mask);
            const QByteArray bytes("model bytes");
            const QString digest = QString::fromLatin1(QCryptographicHash::hash(bytes, QCryptographicHash::Sha256).toHex());
            const auto result = cloakframe::saveModelFile(q(root / "models" / "model.onnx"), bytes, digest);
            ::umask(previous);
            const char *names[] = {"saved", "folder_not_private", "write_failed", "content_mismatch", "publish_failed"};
            runs.append(QJsonObject{{"umask", octal(mask)},
                {"result", names[static_cast<int>(result)]},
                {"folder_mode", modeOf(root / "models")},
                {"file_exists", fs::exists(root / "models" / "model.onnx")}});
        }
        writeJson(g_fixtures / "defects" / "D5" / "observed_1121.json",
            QJsonObject{{"function", "saveModelFile(<new folder>/model.onnx, bytes, sha256)"}, {"runs", runs}});
    }

    void d6Encoding()
    {
        const auto out = g_work / "D6" / "out";
        fs::remove_all(g_work / "D6");
        QStringList inputs;
        for (const auto &entry : fs::directory_iterator(g_fixtures / "defects" / "D6"))
        {
            const auto ext = entry.path().extension().string();
            if (ext == ".jpg" || ext == ".webp" || ext == ".png" || ext == ".tif")
            {
                inputs << q(entry.path());
            }
        }
        inputs.sort();
        cloakframe::ProcessingRequest request;
        request.inputs = inputs;
        request.outputDirectory = q(out);
        request.detectFaces = false;
        request.recursive = false;
        auto run = runWorker(std::move(request));
        run["output_dir"] = q(out);
        writeJson(g_work / "D6" / "worker.json", run);
    }

    void d7LoadBeforeScan()
    {
        const auto root = g_work / "D7";
        fs::remove_all(root);
        fs::create_directories(root / "input");
        QFile note(q(root / "input" / "notes.txt"));
        note.open(QIODevice::WriteOnly);
        note.write("not media");
        note.close();

        cloakframe::ProcessingRequest request;
        request.inputs = {q(root / "input")};
        request.outputDirectory = q(root / "out");
        request.modelPath = q(root / "missing-model.onnx");
        request.faceModelKind = cloakframe::FaceModelKind::Yolo5Face;
        request.detectFaces = true;
        auto run = runWorker(std::move(request));
        run["input"] = "folder with one .txt file and no media";
        run["model_path"] = "a model file that does not exist";
        writeJson(g_fixtures / "defects" / "D7" / "observed_1121.json", run);
    }

    void d11Omitted()
    {
        const cv::Mat crowd =
            cv::imread((g_fixtures / "detect" / "inputs" / "crowd-280-faces.png").string(), cv::IMREAD_COLOR);
        cloakframe::Yolo5FaceDetector detector((g_models / "yolov5n_face.onnx").string(), false);
        QJsonArray runs;
        for (const float score : {0.1F, 0.25F, 0.5F})
        {
            const auto result = detector.detect(crowd, score, 0.4F);
            runs.append(QJsonObject{{"score_threshold", score},
                {"nms_threshold", 0.4},
                {"detections_after_nms", static_cast<int>(result.detections.size())},
                {"omitted", result.omitted}});
        }
        writeJson(g_fixtures / "defects" / "D11" / "observed_1121.json",
            QJsonObject{{"input", "detect/inputs/crowd-280-faces.png (280 identical 32x44 faces)"},
                {"model", "yolov5n_face.onnx (CPU)"},
                {"limits_1121", QJsonObject{{"candidates_before_nms", 2000}, {"detections_after_nms", 300}}},
                {"runs", runs}});
    }

    void d12TempCopy()
    {
        const auto root = g_work / "D12";
        fs::remove_all(root);
        const auto tmp = root / "tmpdir";
        fs::create_directories(tmp);
        fs::create_directories(root / "input");
        cv::Mat big(3000, 4000, CV_8UC3);
        cv::randu(big, 0, 255);
        cv::imwrite((root / "input" / "large.png").string(), big);
        const auto inputSize = fs::file_size(root / "input" / "large.png");

        const QByteArray previousTmp = qgetenv("TMPDIR");
        qputenv("TMPDIR", QByteArray::fromStdString(tmp.string()));
        std::atomic<bool> stop{false};
        std::set<std::string> seen;
        std::uintmax_t largestSeen = 0;
        std::mutex mutex;
        std::thread watcher(
            [&]
            {
                while (!stop.load())
                {
                    std::error_code ec;
                    for (auto it = fs::recursive_directory_iterator(tmp, ec); !ec && it != fs::recursive_directory_iterator(); it.increment(ec))
                    {
                        std::lock_guard lock(mutex);
                        seen.insert(fs::relative(it->path(), tmp, ec).string());
                        if (it->is_regular_file(ec))
                        {
                            largestSeen = std::max(largestSeen, it->file_size(ec));
                        }
                    }
                    std::this_thread::sleep_for(std::chrono::microseconds(200));
                }
            });
        cloakframe::ProcessingRequest request;
        request.inputs = {q(root / "input" / "large.png")};
        request.outputDirectory = q(root / "out");
        request.detectFaces = false;
        auto run = runWorker(std::move(request));
        stop = true;
        watcher.join();
        qputenv("TMPDIR", previousTmp);

        QJsonArray names;
        for (const auto &name : seen)
        {
            names.append(QString::fromStdString(name));
        }
        writeJson(g_fixtures / "defects" / "D12" / "observed_1121.json",
            QJsonObject{{"input", "4000x3000 random PNG, detection off"},
                {"input_bytes", static_cast<qint64>(inputSize)},
                {"tmpdir_entries_seen_during_run", names},
                {"largest_tmpdir_file_bytes", static_cast<qint64>(largestSeen)},
                {"tmpdir_empty_after_run", fs::is_empty(tmp)},
                {"outcome", run["outcome"]}});
    }

    // Every allowlisted tag, the identifying tags 1.12.1 must drop, GPS, and one allowlisted
    // tag past the 128-byte entry limit, written with Exiv2 into a small JPEG.
    void metadataAllowlist()
    {
        const auto dir = g_fixtures / "metadata";
        fs::create_directories(dir);
        const auto source = dir / "rich-exif.jpg";
        const cv::Mat image(48, 64, CV_8UC3, cv::Scalar(90, 120, 150));
        cv::imwrite(source.string(), image, {cv::IMWRITE_JPEG_QUALITY, 90});
        {
            auto meta = Exiv2::ImageFactory::open(source.string());
            meta->readMetadata();
            auto &e = meta->exifData();
            e["Exif.Image.Make"] = "CloakCam";
            e["Exif.Image.Model"] = "CF-1";
            e["Exif.Image.Orientation"] = static_cast<uint16_t>(6);
            e["Exif.Image.DateTime"] = "2026:10:16 12:34:56";
            e["Exif.Image.Artist"] = "PrivateArtist";
            e["Exif.Image.Copyright"] = "PrivateCopyright";
            e["Exif.Image.Software"] = "PrivateSoftware 1.0";
            e["Exif.Image.HostComputer"] = "PrivateHost";
            e["Exif.Image.ImageDescription"] = "PrivateDescription";
            e["Exif.Photo.DateTimeOriginal"] = "2026:10:16 12:34:50";
            e["Exif.Photo.DateTimeDigitized"] = "2026:10:16 12:34:51";
            e["Exif.Photo.OffsetTime"] = "+09:00";
            e["Exif.Photo.OffsetTimeOriginal"] = "+09:00";
            e["Exif.Photo.OffsetTimeDigitized"] = "+09:00";
            e["Exif.Photo.SubSecTime"] = "123";
            e["Exif.Photo.SubSecTimeOriginal"] = "120";
            e["Exif.Photo.SubSecTimeDigitized"] = "121";
            e["Exif.Photo.ExposureTime"] = Exiv2::URational(1, 125);
            e["Exif.Photo.FNumber"] = Exiv2::URational(28, 10);
            e["Exif.Photo.ExposureProgram"] = static_cast<uint16_t>(2);
            e["Exif.Photo.ISOSpeedRatings"] = static_cast<uint16_t>(200);
            e["Exif.Photo.ExposureBiasValue"] = Exiv2::Rational(-1, 3);
            e["Exif.Photo.MeteringMode"] = static_cast<uint16_t>(5);
            e["Exif.Photo.Flash"] = static_cast<uint16_t>(16);
            e["Exif.Photo.FocalLength"] = Exiv2::URational(50, 1);
            e["Exif.Photo.FocalLengthIn35mmFilm"] = static_cast<uint16_t>(75);
            e["Exif.Photo.ExposureMode"] = static_cast<uint16_t>(0);
            e["Exif.Photo.WhiteBalance"] = static_cast<uint16_t>(0);
            e["Exif.Photo.SceneCaptureType"] = static_cast<uint16_t>(0);
            e["Exif.Photo.ColorSpace"] = static_cast<uint16_t>(1);
            e["Exif.Photo.LensMake"] = "CloakLens";
            e["Exif.Photo.LensModel"] = "CL 50mm F1.8";
            Exiv2::URationalValue spec;
            spec.read("50/1 50/1 18/10 18/10");
            e["Exif.Photo.LensSpecification"] = spec;
            e["Exif.Photo.ApertureValue"] = Exiv2::URational(297, 100);
            e["Exif.Photo.ShutterSpeedValue"] = Exiv2::Rational(697, 100);
            e["Exif.Photo.BodySerialNumber"] = "PrivateBodySerial";
            e["Exif.Photo.LensSerialNumber"] = "PrivateLensSerial";
            e["Exif.Photo.CameraOwnerName"] = "PrivateOwner";
            e["Exif.Photo.ImageUniqueID"] = "PrivateUniqueID0123456789abcdef";
            e["Exif.Photo.UserComment"] = "charset=Ascii PrivateUserComment";
            Exiv2::DataValue makerNote(Exiv2::undefined);
            makerNote.read("80 114 105 118 97 116 101 77 97 107 101 114");
            e["Exif.Photo.MakerNote"] = makerNote;
            Exiv2::DataValue gpsVersion(Exiv2::unsignedByte);
            gpsVersion.read("2 3 0 0");
            e["Exif.GPSInfo.GPSVersionID"] = gpsVersion;
            e["Exif.GPSInfo.GPSLatitudeRef"] = "N";
            e["Exif.GPSInfo.GPSLatitude"] = "37/1 33/1 59/1";
            e["Exif.GPSInfo.GPSLongitudeRef"] = "E";
            e["Exif.GPSInfo.GPSLongitude"] = "126/1 58/1 41/1";
            Exiv2::DataValue altitudeRef(Exiv2::unsignedByte);
            altitudeRef.read("0");
            e["Exif.GPSInfo.GPSAltitudeRef"] = altitudeRef;
            e["Exif.GPSInfo.GPSAltitude"] = Exiv2::URational(38, 1);
            e["Exif.GPSInfo.GPSTimeStamp"] = "3/1 34/1 50/1";
            e["Exif.GPSInfo.GPSDateStamp"] = "2026:10:16";
            meta->xmpData()["Xmp.dc.creator"] = "PrivateXmpCreator";
            meta->iptcData()["Iptc.Application2.Byline"] = "PrivateIptcByline";
            meta->setComment("PrivateJpegComment");
            meta->writeMetadata();
        }
        const auto oversized = dir / "oversized-entry.jpg";
        cv::imwrite(oversized.string(), image, {cv::IMWRITE_JPEG_QUALITY, 90});
        {
            auto meta = Exiv2::ImageFactory::open(oversized.string());
            meta->readMetadata();
            meta->exifData()["Exif.Image.Make"] = std::string(200, 'M');
            meta->exifData()["Exif.Image.Model"] = "CF-1";
            meta->writeMetadata();
        }

        QJsonObject results;
        for (const auto &[name, src, gps] : std::vector<std::tuple<std::string, fs::path, bool>>{
                 {"rich-exif.preserve.jpg", source, false},
                 {"rich-exif.preserve-gps.jpg", source, true},
                 {"oversized-entry.preserve.jpg", oversized, false}})
        {
            const auto destination = g_work / "metadata" / name;
            fs::create_directories(destination.parent_path());
            cv::imwrite(destination.string(), image, {cv::IMWRITE_JPEG_QUALITY, 90});
            results[QString::fromStdString(name)] = cloakframe::copyMetadata(src, destination, true, gps);
        }
        writeJson(g_work / "metadata" / "copy.json", results);
    }

    std::optional<cloakframe::FfmpegTools> tools()
    {
        QString error;
        auto found = cloakframe::locateFfmpegTools(&error);
        if (!found)
        {
            std::fprintf(stderr, "ffmpeg not found: %s\n", error.toUtf8().constData());
        }
        return found;
    }

    int decodeFrameIndex(const cv::Mat &bgr, int sourceWidth, int sourceHeight)
    {
        // Matches gen_inputs.frame_index_frame: blocks are sized from the source frame, so a
        // scaled preview is mapped back before sampling.
        const double sx = static_cast<double>(bgr.cols) / sourceWidth;
        const double sy = static_cast<double>(bgr.rows) / sourceHeight;
        const int block = std::max(8, std::min(sourceWidth, sourceHeight) / 12);
        int value = 0;
        for (int bit = 0; bit < 16; ++bit)
        {
            const int bx = static_cast<int>(((bit % 8) * block + block / 2) * sx);
            const int by = static_cast<int>(((bit / 8) * block + block / 2) * sy);
            const auto p = bgr.at<cv::Vec3b>(by, bx);
            if ((p[0] + p[1] + p[2]) / 3 > 128)
            {
                value |= 1 << bit;
            }
        }
        return value;
    }

    QJsonObject processOne(const cloakframe::FfmpegTools &tools,
        const fs::path &source,
        const fs::path &destination,
        const cloakframe::VideoDetectFn &detect = {},
        cloakframe::AnonymizationMethod method = cloakframe::AnonymizationMethod::Mosaic)
    {
        fs::create_directories(destination.parent_path());
        fs::remove(destination);
        const auto info = cloakframe::probeVideo(tools, q(source));
        if (!info)
        {
            return {{"error", "probe failed"}};
        }
        cloakframe::VideoProcessOptions options;
        options.hardwareEncoder = false;
        options.method = method;
        std::atomic<bool> cancelled{false};
        const auto noDetect = [](const cv::Mat &)
        {
            return cloakframe::FaceDetections{};
        };
        const auto result = cloakframe::processVideo(tools,
            q(source),
            q(destination),
            *info,
            options,
            detect ? detect : cloakframe::VideoDetectFn(noDetect),
            cancelled);
        const char *names[] = {"completed", "skipped", "cancelled", "failed"};
        QJsonArray spans;
        for (const auto &span : result.uncoveredSpans)
        {
            spans.append(QJsonArray{span.firstFrame, span.lastFrame});
        }
        return {{"status", names[static_cast<int>(result.status)]},
            {"error", result.error},
            {"frame_count", result.frameCount},
            {"track_count", result.trackCount},
            {"uncovered_frames", result.uncoveredFrames},
            {"uncovered_spans", spans},
            {"dropped_tracks", result.droppedTracks},
            {"encoder", result.encoderName},
            {"output", q(destination)},
            {"output_exists", fs::exists(destination)},
            {"output_mode", modeOf(destination)}};
    }

    void videoDefects()
    {
        const auto found = tools();
        if (!found)
        {
            std::exit(1);
        }
        const auto &t = *found;
        QJsonObject runs;
        for (const auto &[id, names] : std::vector<std::pair<std::string, std::vector<std::string>>>{
                 {"D1", {"bt709-tv-1080p.mp4", "bt709-pc-1080p.mp4", "bt601-tv-480p.mp4"}},
                 {"P-3", {"cfr-2997-offset-bframes.mp4", "cfr-23976.mp4", "vfr-30-15.mp4", "rotated-90.mp4"}},
                 {"P-5", {"odd-641x361-audio-longer.mp4", "audio-starts-early.mp4"}}})
        {
            for (const auto &name : names)
            {
                const auto source = g_fixtures / "defects" / id / name;
                runs[QString::fromStdString(id + "/" + name)] =
                    processOne(t, source, g_work / "video" / id / name);
            }
        }
        writeJson(g_work / "video" / "process.json", runs);
    }

    // Frame indices the analysis pass (VideoFrameReader) and the review preview
    // (videoPreviewFrameArguments) produce for the same frame numbers.
    void p3FrameSelection()
    {
        const auto found = tools();
        const auto &t = *found;
        QJsonObject cases;
        for (const std::string name : {"P-3/cfr-2997-offset-bframes.mp4",
                 "P-3/cfr-23976.mp4",
                 "P-3/vfr-30-15.mp4",
                 "P-3/rotated-90.mp4",
                 "P-5/audio-starts-early.mp4"})
        {
            const auto source = g_fixtures / "defects" / name;
            const auto info = cloakframe::probeVideo(t, q(source));
            QJsonObject entry{{"probe", QJsonObject{{"width", info->width},
                                            {"height", info->height},
                                            {"rotation", info->rotation},
                                            {"fps_num", info->fpsNum},
                                            {"fps_den", info->fpsDen},
                                            {"start_time_seconds", info->startTimeSeconds},
                                            {"is_vfr", info->isVfr},
                                            {"estimated_frame_count", info->estimatedFrameCount}}}};
            const int srcW = info->width;
            const int srcH = info->height;
            const bool swapped = info->rotation == 90 || info->rotation == 270 || info->rotation == -90;

            cloakframe::VideoFrameReader reader;
            QJsonArray analysis;
            if (reader.open(t, q(source), *info, 960))
            {
                cv::Mat frame;
                while (reader.readFrame(frame))
                {
                    cv::Mat upright = frame;
                    if (swapped)
                    {
                        // The reader returns display-oriented frames; undo the rotation to read the index blocks.
                        cv::rotate(frame, upright, info->rotation == 90 ? cv::ROTATE_90_CLOCKWISE : cv::ROTATE_90_COUNTERCLOCKWISE);
                    }
                    analysis.append(decodeFrameIndex(upright, srcW, srcH));
                }
            }
            entry["analysis_reader_indices"] = analysis;

            cloakframe::VideoReviewRequest request;
            request.sourcePath = q(source);
            request.ffmpegPath = t.ffmpegPath;
            request.frameSize = QSize(info->displayWidth(), info->displayHeight());
            request.fps = info->fps();
            request.fpsNum = info->fpsNum;
            request.fpsDen = info->fpsDen;
            request.startTimeSeconds = info->startTimeSeconds;
            request.frameCount = analysis.size();
            QJsonObject preview;
            for (const int frame : {0, 1, 2, 23, 24, 29, 30, 31, 45, 59, 71, 89})
            {
                if (frame >= analysis.size())
                {
                    continue;
                }
                QProcess process;
                process.start(t.ffmpegPath, cloakframe::videoPreviewFrameArguments(request, frame));
                process.waitForFinished(30000);
                const QByteArray png = process.readAllStandardOutput();
                const cv::Mat decoded = cv::imdecode(std::vector<uchar>(png.begin(), png.end()), cv::IMREAD_COLOR);
                if (decoded.empty())
                {
                    preview[QString::number(frame)] = QJsonValue();
                    continue;
                }
                cv::Mat upright = decoded;
                if (swapped)
                {
                    cv::rotate(decoded, upright, info->rotation == 90 ? cv::ROTATE_90_CLOCKWISE : cv::ROTATE_90_COUNTERCLOCKWISE);
                }
                preview[QString::number(frame)] = decodeFrameIndex(upright, srcW, srcH);
            }
            entry["preview_indices_by_frame"] = preview;
            cases[QString::fromStdString(name)] = entry;
        }
        writeJson(g_work / "video" / "p3-selection.json", cases);
    }

    // A face missing in frames 30-54: longer than the 2/3 s interpolation window but shorter
    // than the 1 s a track survives, so the tracker reports it as uncovered. 1.12.1 publishes
    // the file anyway.
    void p1UnresolvedGapIsPublished()
    {
        const auto found = tools();
        const auto &t = *found;
        const auto source = g_fixtures / "defects" / "P-3" / "cfr-2997-offset-bframes.mp4";
        const auto detect = [](const cv::Mat &frame)
        {
            const int index = decodeFrameIndex(frame, 640, 360);
            if (index >= 30 && index < 55)
            {
                return cloakframe::FaceDetections{};
            }
            return cloakframe::FaceDetections{{cv::Rect2f(400.0F, 150.0F, 80.0F, 80.0F), 0.9F}};
        };
        // Fill makes masked frames obvious against the gray background.
        auto run = processOne(t, source, g_work / "video" / "P-1" / "gap.mp4", detect, cloakframe::AnonymizationMethod::Fill);
        run["scenario"] = "detector reports a face at (400,150,80,80) except frames 30-54; fill; review off";
        writeJson(g_work / "video" / "p1-video.json", run);

        const auto root = g_work / "P-1-photo";
        fs::remove_all(root);
        cloakframe::ProcessingRequest request;
        request.inputs = {q(g_fixtures / "detect" / "inputs" / "crowd-280-faces.png")};
        request.outputDirectory = q(root / "out");
        request.modelPath = q(g_models / "yolov5n_face.onnx");
        request.faceModelKind = cloakframe::FaceModelKind::Yolo5Face;
        request.scoreThreshold = 0.1F;
        auto photo = runWorker(std::move(request));
        photo["scenario"] = "crowd-280-faces.png, YOLO5Face-n, score 0.1 (D11 omits candidates)";
        photo["output_exists"] = fs::exists(root / "out" / "crowd-280-faces.png");
        writeJson(g_work / "P-1-photo.json", photo);
    }

    cloakframe::Track constantTrack(int first, int last, float size, float x = 100.0F, float y = 100.0F)
    {
        cloakframe::Track track;
        track.id = 1;
        for (int frame = first; frame <= last; ++frame)
        {
            track.boxes.push_back({frame, cv::Rect2f(x, y, size, size), 0.9F});
        }
        return track;
    }

    bool contains(const cv::Rect2f &outer, const cv::Rect2f &inner)
    {
        constexpr float e = 0.001F;
        return outer.x <= inner.x + e && outer.y <= inner.y + e
               && outer.x + outer.width >= inner.x + inner.width - e
               && outer.y + outer.height >= inner.y + inner.height - e;
    }

    void p6Tracking()
    {
        // (a) A size jump inside a gap: interpolation covers it with the union of the two ends,
        // then smoothing averages that union with the smaller neighbours.
        auto track = constantTrack(0, 9, 40.0F);
        auto after = constantTrack(13, 22, 80.0F, 80.0F, 80.0F);
        track.boxes.insert(track.boxes.end(), after.boxes.begin(), after.boxes.end());
        const auto input = track;
        cloakframe::interpolateGaps(track, 20);
        const auto interpolated = track;
        cloakframe::smoothTrack(track, 2);

        QJsonArray frames;
        for (std::size_t i = 0; i < track.boxes.size(); ++i)
        {
            const auto &before = interpolated.boxes[i];
            const auto &smoothed = track.boxes[i];
            if (!before.interpolated && (before.frame < 8 || before.frame > 15))
            {
                continue;
            }
            frames.append(QJsonObject{{"frame", before.frame},
                {"interpolated", before.interpolated},
                {"box_after_interpolation", rect(before.box)},
                {"box_after_smoothing", rect(smoothed.box)},
                {"smoothed_contains_interpolated", contains(smoothed.box, before.box)}});
        }
        QJsonArray inputBoxes;
        for (const auto &b : input.boxes)
        {
            inputBoxes.append(QJsonObject{{"frame", b.frame}, {"box", rect(b.box)}, {"score", b.score}});
        }

        // (b) A one-frame shot between two different scenes.
        const auto solid = [](int v)
        {
            return cv::Mat(270, 480, CV_8UC1, cv::Scalar(v));
        };
        QJsonArray sceneRuns;
        for (const auto &[label, sequence] : std::vector<std::pair<QString, std::vector<std::pair<int, int>>>>{
                 {"A x10, B x1, C x10", {{0, 10}, {255, 1}, {110, 10}}},
                 {"A x10, B x1, C x1, D x10", {{0, 10}, {255, 1}, {110, 1}, {200, 10}}},
                 {"A x10, B x2, C x10", {{0, 10}, {255, 2}, {110, 10}}}})
        {
            cloakframe::SceneCutDetector detector;
            QJsonArray pushed;
            for (const auto &[value, count] : sequence)
            {
                for (int i = 0; i < count; ++i)
                {
                    detector.push(solid(value));
                }
                pushed.append(QJsonObject{{"solid_value", value}, {"repeat", count}});
            }
            QJsonArray cuts;
            for (const int cut : detector.finish().frames())
            {
                cuts.append(cut);
            }
            sceneRuns.append(QJsonObject{{"label", label},
                {"frame_size", QJsonArray{480, 270}},
                {"pushed", pushed},
                {"cuts", cuts}});
        }

        writeJson(g_fixtures / "defects" / "P-6" / "observed_1121.json",
            QJsonObject{{"a_smoothing_shrinks_interpolated_boxes",
                            QJsonObject{{"input_boxes", inputBoxes},
                                {"calls", "interpolateGaps(track, 20); smoothTrack(track, 2)"},
                                {"frames", frames}}},
                {"b_scene_cut_during_confirmation", sceneRuns}});
    }
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc != 4)
    {
        std::fprintf(stderr, "usage: %s <fixtures dir> <models dir> <work dir>\n", argv[0]);
        return 2;
    }
    g_fixtures = argv[1];
    g_models = argv[2];
    g_work = argv[3];
    fs::create_directories(g_work);

    d2Icc();
    d3Mosaic();
    d4Permissions();
    d5ModelStore();
    d6Encoding();
    d7LoadBeforeScan();
    d11Omitted();
    d12TempCopy();
    videoDefects();
    p3FrameSelection();
    p1UnresolvedGapIsPublished();
    p6Tracking();
    metadataAllowlist();
    return 0;
}
