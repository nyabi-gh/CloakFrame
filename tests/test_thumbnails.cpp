#include "cloakframe/ThumbnailLoader.hpp"

#include <QApplication>
#include <QElapsedTimer>
#include <QListWidget>
#include <QSemaphore>
#include <QThread>

#include <atomic>
#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication app(argc, argv);
    QListWidget list;
    QSemaphore release;
    std::atomic<int> started{0};
    cloakframe::ThumbnailLoader loader(&list,
        [&](const QString &)
        {
            ++started;
            release.acquire();
            QImage image(8, 8, QImage::Format_RGB32);
            image.fill(Qt::red);
            return image;
        });
    const auto waitUntil = [&](const auto &predicate)
    {
        QElapsedTimer timer;
        timer.start();
        while (!predicate() && timer.elapsed() < 2000)
        {
            app.processEvents();
            QThread::msleep(1);
        }
        assert(predicate());
    };
    for (int i = 0; i < 100; ++i)
        list.addItem(QString::number(i));
    waitUntil(
        [&]
        {
            return started == 2;
        });
    assert(list.count() == 100); // All rows are available while both decoders remain blocked.
    list.clear();
    list.addItem("replacement");
    release.release(2);
    waitUntil(
        [&]
        {
            return started == 3;
        });
    assert(list.item(0)->icon().isNull()); // Old row results must not land on the new row.
    release.release();
    waitUntil(
        [&]
        {
            return !list.item(0)->icon().isNull();
        });
    assert(started == 3); // The 98 removed, unstarted rows were discarded.
    return 0;
}
