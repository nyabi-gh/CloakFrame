#pragma once

#include <QImage>
#include <QObject>
#include <QThreadPool>

#include <functional>

class QListWidget;

namespace cloakframe
{
    // Schedules from the live model, so removed rows never build up a stale work queue.
    class ThumbnailLoader final : public QObject
    {
    public:
        using Reader = std::function<QImage(const QString &)>;
        explicit ThumbnailLoader(QListWidget *list, Reader reader = {});
        ~ThumbnailLoader() override;
        void schedule();

    private:
        QListWidget *list_;
        QThreadPool pool_;
        Reader reader_;
        int active_ = 0;
    };
}
