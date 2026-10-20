// Runs CloakFrame 1.12.1's photo detection on the benchmark images (docs/M3_BENCHMARK.md) and
// writes the detections in the JSON form of the engine's benchmark example.
//
// Usage: cloakframe_bench_legacy <models dir> <manifest> <images dir> <out>
//            <yolo5face|yunet|off> <plates on|off> <score> <nms>

#include "cloakframe/ImageIo.hpp"
#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"
#include "cloakframe/YuNetFaceDetector.hpp"

#include <QCoreApplication>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

#include <opencv2/imgcodecs.hpp>

#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <memory>
#include <string>

namespace fs = std::filesystem;

namespace
{
    double rounded(double v, int places)
    {
        const double scale = std::pow(10.0, places);
        return std::round(v * scale) / scale;
    }

    QJsonObject toJson(const cloakframe::FaceDetection &d, const char *kind, const char *model)
    {
        return QJsonObject{{"kind", kind},
            {"model", model},
            {"score", rounded(d.score, 4)},
            {"box",
                QJsonArray{rounded(d.box.x, 1), rounded(d.box.y, 1), rounded(d.box.width, 1),
                    rounded(d.box.height, 1)}}};
    }
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc != 9)
    {
        std::fprintf(stderr,
            "usage: %s <models dir> <manifest> <images dir> <out> <yolo5face|yunet|off> "
            "<plates on|off> <score> <nms>\n",
            argv[0]);
        return 2;
    }
    const fs::path models = argv[1];
    const fs::path images = argv[3];
    const std::string faces = argv[5];
    const bool plates = std::string(argv[6]) == "on";
    const float score = std::strtof(argv[7], nullptr);
    const float nms = std::strtof(argv[8], nullptr);

    QFile manifestFile(QString::fromStdString(argv[2]));
    if (!manifestFile.open(QIODevice::ReadOnly))
    {
        std::fprintf(stderr, "cannot read %s\n", argv[2]);
        return 1;
    }
    const QJsonArray entries = QJsonDocument::fromJson(manifestFile.readAll())["images"].toArray();

    // CPU sessions: on Apple silicon 1.12.1's CoreML results equal its CPU results
    // (docs/M0_ORT_SPIKE.md), and YuNet always ran on OpenCV's CPU backend.
    std::unique_ptr<cloakframe::Detector> face;
    const char *faceModel = nullptr;
    if (faces == "yolo5face")
    {
        face = std::make_unique<cloakframe::Yolo5FaceDetector>((models / "yolov5n_face.onnx").string(), false);
        faceModel = "yolo5face-n";
    }
    else if (faces == "yunet")
    {
        face = std::make_unique<cloakframe::YuNetFaceDetector>((models / "face_detection_yunet_2023mar.onnx").string());
        faceModel = "yunet";
    }
    std::unique_ptr<cloakframe::Detector> plate;
    if (plates)
    {
        plate = std::make_unique<cloakframe::PlateDetector>(
            (models / "yolo-v9-t-512-license-plates-end2end.onnx").string(), false);
    }

    QJsonArray results;
    const auto started = std::chrono::steady_clock::now();
    for (const auto &value : entries)
    {
        const QString name = value.toObject()["name"].toString();
        const fs::path path = images / name.toStdString();
        QJsonObject result{{"name", name}, {"views", 1}};
        // ProcessorWorker's photo path: decode unchanged, apply EXIF orientation, convert to
        // 8-bit BGR, then the face and the plate detector with the run's one score threshold.
        cv::Mat image = cloakframe::imreadUnicode(path, cv::IMREAD_UNCHANGED);
        if (image.empty())
        {
            result["failure"] = "unreadable";
            results.append(result);
            continue;
        }
        cloakframe::applyOrientation(image, cloakframe::readExifOrientation(path));
        const cv::Mat bgr = cloakframe::toDetectionBgr(image);
        QJsonArray detections;
        int omitted = 0;
        if (face)
        {
            const auto found = face->detect(bgr, score, nms);
            for (const auto &d : found.detections)
                detections.append(toJson(d, "face", faceModel));
            omitted += found.omitted;
        }
        if (plate)
        {
            const auto found = plate->detect(bgr, score, nms);
            for (const auto &d : found.detections)
                detections.append(toJson(d, "plate", "plate"));
            omitted += found.omitted;
        }
        result["failure"] = QJsonValue::Null;
        result["unresolved"] = omitted > 0 ? 1 : 0;
        result["omitted"] = omitted;
        result["detections"] = detections;
        results.append(result);
    }
    const double seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count();

    const QJsonObject thresholds{{"score", rounded(score, 4)}, {"nms", rounded(nms, 4)}};
    const QJsonObject doc{{"detector", "1.12.1"},
        {"faces", faceModel ? QJsonValue(faceModel) : QJsonValue()},
        {"plates", plates},
        {"thresholds", QJsonObject{{"face", thresholds}, {"plate", thresholds}}},
        {"tiles", false},
        {"backends", QJsonArray{"CPU"}},
        {"workers", 1},
        {"seconds", seconds}};
    QFile out(QString::fromStdString(argv[4]));
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate))
    {
        std::fprintf(stderr, "cannot write %s\n", argv[4]);
        return 1;
    }
    // One image per line, as the engine's example writes, so the recorded results stay small.
    QByteArray text = QJsonDocument(doc).toJson(QJsonDocument::Compact);
    text.chop(1);
    text += ",\"images\":[\n";
    for (qsizetype i = 0; i < results.size(); ++i)
    {
        if (i > 0)
            text += ",\n";
        text += QJsonDocument(results[i].toObject()).toJson(QJsonDocument::Compact);
    }
    text += "\n]}\n";
    out.write(text);
    return 0;
}
