#include "cloakframe/VideoReviewDialog.hpp"

#include <QApplication>
#include <QKeyEvent>
#include <QLabel>
#include <QListWidget>
#include <QPushButton>
#include <QSlider>
#include <QTranslator>

#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    cloakframe::VideoReviewRequest request;
    request.frameSize = QSize(320, 240);
    request.fps = 30000.0 / 1001.0;
    request.fpsNum = 30000;
    request.fpsDen = 1001;
    request.startTimeSeconds = 3.25;
    request.frameCount = 120;
    request.tracks.push_back({7, true, {{0, QRectF(20, 20, 40, 40), false}}});
    request.uncoveredSpans = {{7, 90, 95}, {8, 10, 15}, {7, 10, 20}, {7, 45, 50}, {7, 0, 2}};
    cloakframe::VideoReviewDialog dialog(request);
    auto *list = dialog.findChild<QListWidget *>("videoTracks");
    assert(list != nullptr && list->count() == 1);
    assert(list->item(0)->checkState() == Qt::Checked);
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());
    auto *gaps = dialog.findChild<QListWidget *>("trackingGaps");
    auto *timeline = dialog.findChild<QSlider *>("videoTimeline");
    auto *position = dialog.findChild<QLabel *>("videoFramePosition");
    auto *previousGap = dialog.findChild<QPushButton *>("previousGap");
    auto *nextGap = dialog.findChild<QPushButton *>("nextGap");
    auto *previousFrame = dialog.findChild<QPushButton *>("previousFrame");
    auto *nextFrame = dialog.findChild<QPushButton *>("nextFrame");
    assert(gaps && timeline && position && previousGap && nextGap && previousFrame && nextFrame);
    assert(gaps->count() == 5);
    assert(!previousGap->isEnabled() && !previousFrame->isEnabled());
    assert(gaps->item(1)->text().contains("0:00.334–0:00.501"));
    assert(gaps->item(1)->text().contains("Frames 11–16"));
    gaps->setCurrentRow(2);
    assert(timeline->value() == 10);
    assert(gaps->currentRow() == 2);
    assert(position->text().contains("0:00.334"));
    assert(position->text().contains("Frame 11 / 120"));
    nextGap->click();
    assert(timeline->value() == 45);
    nextGap->click();
    assert(timeline->value() == 90 && !nextGap->isEnabled());
    previousGap->click();
    assert(timeline->value() == 45);
    timeline->setValue(50);
    previousGap->click();
    assert(timeline->value() == 45);
    previousGap->click();
    assert(timeline->value() == 10);
    previousGap->click();
    assert(timeline->value() == 0 && !previousGap->isEnabled());
    previousFrame->click();
    assert(timeline->value() == 0);
    nextFrame->click();
    assert(timeline->value() == 1);
    timeline->setValue(119);
    nextFrame->click();
    assert(timeline->value() == 119 && !nextFrame->isEnabled());

    dialog.show();
    dialog.activateWindow();
    application.processEvents();
    timeline->setValue(45);
    for (QWidget *target : {static_cast<QWidget *>(&dialog),
             static_cast<QWidget *>(timeline),
             static_cast<QWidget *>(gaps)})
    {
        target->setFocus();
        application.processEvents();
        const int before = timeline->value();
        QKeyEvent right(QEvent::KeyPress, Qt::Key_Right, Qt::NoModifier);
        QApplication::sendEvent(target, &right);
        assert(timeline->value() == before + 1);
        QKeyEvent left(QEvent::KeyPress, Qt::Key_Left, Qt::NoModifier);
        QApplication::sendEvent(target, &left);
        assert(timeline->value() == before);
    }
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());
    assert(dialog.reviewResult().addedTracks.isEmpty());

    auto noGapRequest = request;
    noGapRequest.uncoveredSpans.clear();
    noGapRequest.frameCount = 1;
    cloakframe::VideoReviewDialog noGaps(noGapRequest);
    assert(noGaps.findChild<QListWidget *>("trackingGaps")->count() == 0);
    for (const auto *name : {"previousGap", "nextGap", "previousFrame", "nextFrame"})
    {
        assert(!noGaps.findChild<QPushButton *>(name)->isEnabled());
    }
    const QString screenshot = qEnvironmentVariable("CLOAKFRAME_TEST_SCREENSHOT");
    if (!screenshot.isEmpty())
    {
        QTranslator translator;
        const QString translation = qEnvironmentVariable("CLOAKFRAME_TEST_TRANSLATION");
        if (!translation.isEmpty())
        {
            assert(translator.load(translation));
            application.installTranslator(&translator);
        }
        cloakframe::VideoReviewDialog preview(request);
        preview.show();
        application.processEvents();
        assert(preview.grab().save(screenshot));
    }

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
