#include "cloakframe/VideoReviewDialog.hpp"

#include <QApplication>
#include <QLabel>
#include <QListWidget>

#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    cloakframe::VideoReviewRequest request;
    request.frameSize = QSize(320, 240);
    request.fps = 30.0;
    request.fpsNum = 30;
    request.frameCount = 30;
    request.tracks.push_back({7, true, {{0, QRectF(20, 20, 40, 40), false}}});
    request.uncoveredSpans.push_back({7, 10, 15});
    cloakframe::VideoReviewDialog dialog(request);
    auto *list = dialog.findChild<QListWidget *>();
    assert(list != nullptr && list->count() == 1);
    assert(list->item(0)->checkState() == Qt::Checked);
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());

    bool inclusionExplained = false;
    bool gapExplained = false;
    for (const auto *label : dialog.findChildren<QLabel *>())
    {
        assert(!label->text().contains("excluded by default"));
        inclusionExplained |= label->text().contains("included by default");
        gapExplained |= label->text().contains("adding a mask does not verify");
    }
    assert(inclusionExplained && gapExplained);
    list->item(0)->setCheckState(Qt::Unchecked);
    assert(dialog.reviewResult().excludedTrackIds == QVector<int>{7});
    list->item(0)->setCheckState(Qt::Checked);
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());
    return 0;
}
