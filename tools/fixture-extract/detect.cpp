// Runs the three 1.12.1 detectors on the detection fixture images and writes their results.
//
// Usage: cloakframe_fixture_detect <models dir> <out dir> <image>...

#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"
#include "cloakframe/YuNetFaceDetector.hpp"

#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QFile>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <memory>

namespace fs = std::filesystem;

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc < 4)
    {
        std::fprintf(stderr, "usage: %s <models dir> <out dir> <image>...\n", argv[0]);
        return 2;
    }
    const fs::path models = argv[1];
    const fs::path out = argv[2];

    struct Model
    {
        const char *name;
        std::unique_ptr<cloakframe::Detector> detector;
    };
    Model detectors[] = {
        {"yolo5face", std::make_unique<cloakframe::Yolo5FaceDetector>((models / "yolov5n_face.onnx").string(), false)},
        {"yunet", std::make_unique<cloakframe::YuNetFaceDetector>((models / "face_detection_yunet_2023mar.onnx").string())},
        {"plate", std::make_unique<cloakframe::PlateDetector>((models / "yolo-v9-t-512-license-plates-end2end.onnx").string(), false)},
    };

    for (int i = 3; i < argc; ++i)
    {
        const fs::path image = argv[i];
        // Decoded with OpenCV as 1.12.1 does; the fixture records the decoded pixels' hash so a
        // different decoder is caught before results are compared.
        const cv::Mat bgr = cv::imread(image.string(), cv::IMREAD_COLOR);
        if (bgr.empty())
        {
            std::fprintf(stderr, "cannot read %s\n", image.c_str());
            return 1;
        }
        // The Rust ORT spike compares against these: OpenCV's decode and its INTER_LINEAR
        // resize to the two model input sizes, rounded the way each detector rounds.
        if (const char *decoded = std::getenv("CFREC_DECODED_DIR"))
        {
            const fs::path dir = decoded;
            fs::create_directories(dir);
            const auto stem = image.stem().string();
            cv::imwrite((dir / (stem + ".png")).string(), bgr);
            for (const int target : {640, 512})
            {
                const float scale = std::min(static_cast<float>(target) / bgr.cols, static_cast<float>(target) / bgr.rows);
                const bool rounded = target == 512;
                const int w = std::max(1, rounded ? static_cast<int>(std::round(bgr.cols * scale)) : static_cast<int>(bgr.cols * scale));
                const int h = std::max(1, rounded ? static_cast<int>(std::round(bgr.rows * scale)) : static_cast<int>(bgr.rows * scale));
                cv::Mat resized;
                cv::resize(bgr, resized, cv::Size(w, h), 0.0, 0.0, cv::INTER_LINEAR);
                cv::imwrite((dir / ("resized_" + std::to_string(target) + "_" + stem + ".png")).string(), resized);
            }
        }
        for (auto &model : detectors)
        {
            for (const float score : {0.1F, 0.25F, 0.5F})
            {
                const auto result = model.detector->detect(bgr, score, 0.4F);
                QJsonArray detections;
                for (const auto &d : result.detections)
                {
                    detections.append(QJsonObject{{"box", QJsonArray{d.box.x, d.box.y, d.box.width, d.box.height}},
                        {"score", d.score},
                        {"roll", d.rollRadians},
                        {"has_pose", d.hasPose}});
                }
                const QJsonObject doc{{"model", model.name},
                    {"image", QString::fromStdString(image.filename().string())},
                    {"decoded_size", QJsonArray{bgr.cols, bgr.rows}},
                    {"score_threshold", score},
                    {"nms_threshold", 0.4},
                    {"backend", model.detector->backendName()},
                    {"omitted", result.omitted},
                    {"detections", detections}};
                const auto dir = out / model.name;
                fs::create_directories(dir);
                char scoreName[16];
                std::snprintf(scoreName, sizeof scoreName, "%.2f", score);
                QFile file(QString::fromStdString((dir / (image.stem().string() + "@" + scoreName + ".json")).string()));
                file.open(QIODevice::WriteOnly | QIODevice::Truncate);
                file.write(QJsonDocument(doc).toJson(QJsonDocument::Indented));
            }
        }
    }
    return 0;
}
