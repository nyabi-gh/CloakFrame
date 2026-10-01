#include "cloakframe/ThumbnailLoader.hpp"

#include "cloakframe/ImageIo.hpp"
#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/PathUtil.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QImageReader>
#include <QListWidget>
#include <QProcess>
#include <QTimer>

namespace cloakframe
{
    namespace
    {
        constexpr int kRequestedRole = Qt::UserRole + 10;
        constexpr int kKeyRole = Qt::UserRole + 11;
        QImage readThumbnail(const QString &path)
        {
            if (isSupportedVideo(pathFromQString(path)))
            {
                const auto tools = locateFfmpegTools();
                if (!tools)
                    return {};
                QProcess process;
                process.setStandardErrorFile(QProcess::nullDevice());
                process.start(tools->ffmpegPath,
                    QStringList{"-v", "error", "-threads", "1"} + ffmpegFileInput(path)
                        + QStringList{"-frames:v",
                            "1",
                            "-vf",
                            "scale=80:80:force_original_aspect_ratio=decrease",
                            "-threads",
                            "1",
                            "-f",
                            "image2pipe",
                            "-c:v",
                            "png",
                            "-"});
                if (!process.waitForStarted(3000) || !process.waitForFinished(3000))
                {
                    process.kill();
                    process.waitForFinished(1000);
                    return {};
                }
                if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
                    return {};
                return QImage::fromData(process.readAllStandardOutput(), "PNG");
            }
            const auto format = sniffImageFormat(pathFromQString(path));
            if (!format)
                return {};
            QImageReader reader(path);
            restrictImageReader(reader, *format);
            reader.setAutoTransform(true);
            const QSize size = reader.size();
            // Some image plugins decode the full image before scaling. Bound that allocation
            // as well; oversized/unknown images keep the placeholder icon.
            if (!size.isValid() || static_cast<qint64>(size.width()) * size.height() > 16'000'000)
                return {};
            reader.setScaledSize(size.scaled(80, 80, Qt::KeepAspectRatio));
            return reader.read().scaled(80, 80, Qt::KeepAspectRatio, Qt::SmoothTransformation);
        }
    }

    ThumbnailLoader::ThumbnailLoader(QListWidget *list, Reader reader)
        : QObject(list)
        , list_(list)
        , reader_(reader ? std::move(reader) : readThumbnail)
    {
        pool_.setMaxThreadCount(2);
        connect(list_->model(),
            &QAbstractItemModel::rowsInserted,
            this,
            [this]
            {
                QTimer::singleShot(0, this, &ThumbnailLoader::schedule);
            });
    }

    ThumbnailLoader::~ThumbnailLoader()
    {
        pool_.waitForDone();
    }

    void ThumbnailLoader::schedule()
    {
        for (int row = 0; active_ < 2 && row < list_->count(); ++row)
        {
            auto *item = list_->item(row);
            if (item->data(kRequestedRole).toBool())
                continue;
            item->setData(kRequestedRole, true);
            // A model index must not be destroyed off the GUI thread, so the job carries a key.
            const quint64 key = ++nextKey_;
            item->setData(kKeyRole, key);
            const QString path = item->text();
            ++active_;
            pool_.start(
                [this, key, path, reader = reader_]
                {
                    const QImage image = reader(path);
                    QMetaObject::invokeMethod(
                        this,
                        [this, key, image]
                        {
                            --active_;
                            for (int index = 0; !image.isNull() && index < list_->count(); ++index)
                            {
                                auto *target = list_->item(index);
                                if (target->data(kKeyRole).toULongLong() == key)
                                {
                                    target->setIcon(QIcon(QPixmap::fromImage(image)));
                                    break;
                                }
                            }
                            schedule();
                        },
                        Qt::QueuedConnection);
                });
        }
    }
}
