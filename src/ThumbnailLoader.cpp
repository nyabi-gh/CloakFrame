#include "cloakframe/ThumbnailLoader.hpp"

#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/PathUtil.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QImageReader>
#include <QListWidget>
#include <QPersistentModelIndex>
#include <QProcess>
#include <QTimer>

namespace cloakframe
{
    namespace
    {
        constexpr int kRequestedRole = Qt::UserRole + 10;
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
                    {"-v",
                        "error",
                        "-threads",
                        "1",
                        "-i",
                        path,
                        "-frames:v",
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
            QImageReader reader(path);
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
            const QPersistentModelIndex index(list_->model()->index(row, 0));
            const QString path = item->text();
            ++active_;
            pool_.start(
                [this, index, path, reader = reader_]
                {
                    const QImage image = reader(path);
                    QMetaObject::invokeMethod(
                        this,
                        [this, index, image]
                        {
                            --active_;
                            if (index.isValid() && !image.isNull())
                                list_->item(index.row())->setIcon(QIcon(QPixmap::fromImage(image)));
                            schedule();
                        },
                        Qt::QueuedConnection);
                });
        }
    }
}
