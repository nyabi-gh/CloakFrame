#include "cloakframe/ModelDownloader.hpp"

#include "cloakframe/ModelCatalog.hpp"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QMessageBox>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProgressDialog>
#include <QUrl>

#include <algorithm>

namespace cloakframe
{
    bool downloadModelWithProgress(
        QWidget *parent, const BuiltinModel &model, const QString &destPath)
    {
        QNetworkAccessManager manager;
        QNetworkRequest request{QUrl(model.url)};
        request.setAttribute(
            QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::NoLessSafeRedirectPolicy);

        QProgressDialog progress(
            QCoreApplication::translate("cloakframe::MainWindow", "Downloading model…"),
            QCoreApplication::translate("cloakframe::MainWindow", "Cancel"),
            0,
            0,
            parent);
        progress.setWindowModality(Qt::WindowModal);
        progress.setMinimumDuration(0);
        progress.setAutoClose(false);
        progress.setAutoReset(false);

        QNetworkReply *reply = manager.get(request);

        const qint64 maxBytes = std::max<qint64>(model.approxBytes * 4, 64LL * 1024 * 1024);
        bool tooLarge = false;

        QObject::connect(reply,
            &QNetworkReply::downloadProgress,
            &progress,
            [&progress, &tooLarge, reply, maxBytes](qint64 received, qint64 total)
            {
                if (received > maxBytes || (total > 0 && total > maxBytes))
                {
                    tooLarge = true;
                    reply->abort();
                    return;
                }
                if (total > 0)
                {
                    progress.setMaximum(100);
                    progress.setValue(static_cast<int>(received * 100 / total));
                }
            });

        QEventLoop loop;
        QObject::connect(reply, &QNetworkReply::finished, &loop, &QEventLoop::quit);
        QObject::connect(&progress, &QProgressDialog::canceled, reply, &QNetworkReply::abort);
        loop.exec();
        progress.close();

        if (tooLarge)
        {
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Download Failed"),
                QCoreApplication::translate("cloakframe::MainWindow",
                    "The download was much larger than expected and was stopped."));
            return false;
        }

        if (reply->error() != QNetworkReply::NoError)
        {
            if (reply->error() != QNetworkReply::OperationCanceledError)
            {
                QMessageBox::warning(parent,
                    QCoreApplication::translate("cloakframe::MainWindow", "Download Failed"),
                    QCoreApplication::translate(
                        "cloakframe::MainWindow", "Could not download the model.\n\n%1")
                        .arg(reply->errorString()));
            }
            return false;
        }

        const QByteArray data = reply->readAll();
        const auto actual =
            QString::fromLatin1(QCryptographicHash::hash(data, QCryptographicHash::Sha256).toHex());
        if (actual.compare(model.sha256, Qt::CaseInsensitive) != 0)
        {
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Download Failed"),
                QCoreApplication::translate("cloakframe::MainWindow",
                    "The downloaded model failed its integrity check and was discarded."));
            return false;
        }

        QDir().mkpath(QFileInfo(destPath).absolutePath());
        const QString tempPath = destPath + ".part";
        QFile file(tempPath);
        if (!file.open(QIODevice::WriteOnly) || file.write(data) != data.size())
        {
            file.remove();
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Download Failed"),
                QCoreApplication::translate(
                    "cloakframe::MainWindow", "Could not save the model file."));
            return false;
        }
        file.close();
        QFile::remove(destPath);
        if (!QFile::rename(tempPath, destPath))
        {
            QFile::remove(tempPath);
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Download Failed"),
                QCoreApplication::translate(
                    "cloakframe::MainWindow", "Could not save the model file."));
            return false;
        }
        return true;
    }

    namespace
    {
        QString builtinModelConsentText(const BuiltinModel &model)
        {
            switch (model.faceKind)
            {
            case FaceModelKind::Yolo5Face:
                return QCoreApplication::translate("cloakframe::MainWindow",
                    "The %1 model isn't on this computer yet.\n\n"
                    "CloakFrame can download it once (%2 MB) from the "
                    "yolov5-face-onnx-inference project on GitHub. The model is based on "
                    "the GPL-3.0-licensed YOLO5Face project and was trained on the "
                    "WIDER FACE dataset, so treat it as non-commercial only. "
                    "Your images are never uploaded.\n\nDownload now?");
            case FaceModelKind::YuNet:
                return QCoreApplication::translate("cloakframe::MainWindow",
                    "The %1 model isn't on this computer yet.\n\n"
                    "CloakFrame can download it once (%2 MB) from the OpenCV Zoo project "
                    "on GitHub (MIT-licensed). "
                    "Your images are never uploaded.\n\nDownload now?");
            case FaceModelKind::Scrfd:
                break;
            }
            return QCoreApplication::translate("cloakframe::MainWindow",
                "The %1 model isn't on this computer yet.\n\n"
                "CloakFrame can download it once (%2 MB) from its source project. "
                "Your images are never uploaded.\n\nDownload now?");
        }
    }

    bool ensureBuiltinModelAvailable(
        QWidget *parent, const BuiltinModel &model, const QString &destPath)
    {
        const auto sizeMb =
            QString::number(static_cast<double>(model.approxBytes) / 1024.0 / 1024.0, 'f', 1);
        const auto answer = QMessageBox::question(parent,
            QCoreApplication::translate("cloakframe::MainWindow", "Download Model"),
            builtinModelConsentText(model).arg(model.fileName, sizeMb),
            QMessageBox::Yes | QMessageBox::No,
            QMessageBox::Yes);
        if (answer != QMessageBox::Yes)
        {
            return false;
        }
        return downloadModelWithProgress(parent, model, destPath);
    }

    bool ensurePlateModelAvailable(QWidget *parent, const QString &destPath)
    {
        const auto &model = plateModel();
        const auto sizeMb =
            QString::number(static_cast<double>(model.approxBytes) / 1024.0 / 1024.0, 'f', 1);
        const auto answer = QMessageBox::question(parent,
            QCoreApplication::translate("cloakframe::MainWindow", "Download Model"),
            QCoreApplication::translate("cloakframe::MainWindow",
                "The license plate detection model isn't on this computer yet.\n\n"
                "CloakFrame can download it once (%1 MB) from the open-image-models "
                "project (MIT-licensed). Your images are never uploaded.\n\nDownload now?")
                .arg(sizeMb),
            QMessageBox::Yes | QMessageBox::No,
            QMessageBox::Yes);
        if (answer != QMessageBox::Yes)
        {
            return false;
        }
        return downloadModelWithProgress(parent, model, destPath);
    }

    bool customModelFileIsAllowed(QWidget *parent, const QString &path)
    {
        const QFileInfo info(path);
        if (!info.exists() || !info.isFile())
        {
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Invalid Model"),
                QCoreApplication::translate(
                    "cloakframe::MainWindow", "Choose an existing ONNX model file."));
            return false;
        }
        if (info.suffix().compare("onnx", Qt::CaseInsensitive) != 0)
        {
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Invalid Model"),
                QCoreApplication::translate(
                    "cloakframe::MainWindow", "The selected model must use the .onnx extension."));
            return false;
        }
        if (info.size() > kMaxCustomModelBytes)
        {
            QMessageBox::warning(parent,
                QCoreApplication::translate("cloakframe::MainWindow", "Model Too Large"),
                QCoreApplication::translate("cloakframe::MainWindow",
                    "The selected ONNX file is larger than 512 MB. "
                    "Choose a smaller SCRFD model."));
            return false;
        }
        return true;
    }

    bool confirmTrustedCustomModel(QWidget *parent, const QString &path)
    {
        const QFileInfo info(path);
        const auto answer = QMessageBox::question(parent,
            QCoreApplication::translate("cloakframe::MainWindow", "Load Custom Model"),
            QCoreApplication::translate("cloakframe::MainWindow",
                "Only load ONNX models from sources you trust.\n\nModel: %1\nSize: %2 "
                "MB\n\nContinue?")
                .arg(info.fileName())
                .arg(QString::number(static_cast<double>(info.size()) / 1024.0 / 1024.0, 'f', 1)),
            QMessageBox::Yes | QMessageBox::No,
            QMessageBox::No);
        return answer == QMessageBox::Yes;
    }

    bool confirmChangedCustomModel(QWidget *parent, const QString &path)
    {
        const QFileInfo info(path);
        const auto answer = QMessageBox::question(parent,
            QCoreApplication::translate("cloakframe::MainWindow", "Model File Changed"),
            QCoreApplication::translate("cloakframe::MainWindow",
                "This file is no longer the model you approved.\n\nModel: %1\nSize: %2 "
                "MB\n\nSomething replaced its contents since you chose it. Continue only if "
                "you replaced it yourself.\n\nLoad it anyway?")
                .arg(info.fileName())
                .arg(QString::number(static_cast<double>(info.size()) / 1024.0 / 1024.0, 'f', 1)),
            QMessageBox::Yes | QMessageBox::No,
            QMessageBox::No);
        return answer == QMessageBox::Yes;
    }
}
