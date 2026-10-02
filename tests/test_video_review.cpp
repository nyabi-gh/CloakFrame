#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoReviewDialog.hpp"

#include <QApplication>
#include <QKeyEvent>
#include <QLabel>
#include <QListWidget>
#include <QMessageBox>
#include <QProcess>
#include <QPushButton>
#include <QSlider>
#include <QTemporaryDir>
#include <QTest>
#include <QTimer>
#include <QTranslator>

#include <cassert>
#include <cstdio>
#include <optional>

namespace
{
    bool userCheckable(const QListWidgetItem *item)
    {
        return item->flags().testFlag(Qt::ItemIsUserCheckable);
    }

    QPushButton *buttonWithText(const QWidget &parent, const QString &text)
    {
        for (auto *button : parent.findChildren<QPushButton *>())
        {
            if (button->text() == text)
            {
                return button;
            }
        }
        return nullptr;
    }

    void answerNextMessageBox(QMessageBox::StandardButton button, int &shown)
    {
        QTimer::singleShot(0,
            [button, &shown]
            {
                auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
                assert(box != nullptr);
                ++shown;
                box->button(button)->click();
            });
    }

    std::optional<cloakframe::VideoReviewRequest> renderableRequest(const QTemporaryDir &temp)
    {
        const auto tools = cloakframe::locateFfmpegTools();
        if (!tools)
        {
            return std::nullopt;
        }
        assert(temp.isValid());
        const QString source = temp.filePath("clip.mp4");
        QProcess generate;
        generate.start(tools->ffmpegPath,
            {"-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30",
                "-t",
                "1",
                "-pix_fmt",
                "yuv420p",
                "-c:v",
                "mpeg4",
                source});
        assert(generate.waitForFinished(60000) && generate.exitCode() == 0);

        cloakframe::VideoReviewRequest request;
        request.sourcePath = source;
        request.ffmpegPath = tools->ffmpegPath;
        request.frameSize = QSize(320, 240);
        request.fps = 30.0;
        request.fpsNum = 30;
        request.fpsDen = 1;
        request.frameCount = 30;
        request.tracks.push_back({7, true, {{0, QRectF(20, 20, 40, 40), false}}});
        request.uncoveredSpans = {{10, 20}};
        request.initialFrame = 0;
        return request;
    }

    // A gap can be acknowledged only after its first, middle and last frames were on screen,
    // and editing the masks takes that away again. Needs FFmpeg to render real frames.
    void testGapChecksFollowWhatWasShown(QApplication &application)
    {
        QTemporaryDir temp;
        const auto request = renderableRequest(temp);
        if (!request)
        {
            std::puts("SKIP gap checks follow what was shown: FFmpeg not found");
            return;
        }
        cloakframe::VideoReviewDialog dialog(*request);
        dialog.show();
        auto *gaps = dialog.findChild<QListWidget *>("trackingGaps");
        auto *timeline = dialog.findChild<QSlider *>("videoTimeline");
        auto *tracks = dialog.findChild<QListWidget *>("videoTracks");
        const auto show = [&](int frame)
        {
            timeline->setValue(frame);
            assert(QTest::qWaitFor(
                [&]
                {
                    return dialog.property("lastViewedFrame").toInt() == frame;
                },
                30000));
            application.processEvents();
        };

        show(10);
        show(20);
        assert(!userCheckable(gaps->item(0)));
        show(15);
        assert(userCheckable(gaps->item(0)));
        gaps->item(0)->setCheckState(Qt::Checked);
        assert(dialog.reviewResult().acknowledgedGapIndices == QVector<int>{0});

        // An edit clears both the check and the record of what was seen before it.
        tracks->item(0)->setCheckState(Qt::Unchecked);
        assert(gaps->item(0)->checkState() == Qt::Unchecked);
        assert(!userCheckable(gaps->item(0)));
        tracks->item(0)->setCheckState(Qt::Checked);
        show(10);
        show(20);
        assert(userCheckable(gaps->item(0)));
        std::puts("gap checks follow what was shown: ok");
    }

    void testManualTrackRemovalAsks()
    {
        QTemporaryDir temp;
        auto request = renderableRequest(temp);
        if (!request)
        {
            std::puts("SKIP manual track removal asks: FFmpeg not found");
            return;
        }
        request->initialFrame = 5;
        cloakframe::VideoReviewDialog dialog(*request);
        dialog.show();
        assert(QTest::qWaitFor(
            [&]
            {
                return dialog.property("lastViewedFrame").toInt() == 5;
            },
            30000));
        auto *canvas = dialog.findChild<QWidget *>("videoCanvas");
        auto *remove = dialog.findChild<QPushButton *>("removeManualTrack");
        dialog.findChild<QPushButton *>("addManualTrack")->click();
        const QPoint centre = canvas->rect().center();
        QTest::mousePress(canvas, Qt::LeftButton, {}, centre - QPoint(30, 30));
        QTest::mouseRelease(canvas, Qt::LeftButton, {}, centre + QPoint(30, 30));
        assert(dialog.reviewResult().addedTracks.size() == 1);
        assert(remove->isEnabled());

        int shown = 0;
        answerNextMessageBox(QMessageBox::No, shown);
        remove->click();
        assert(shown == 1 && dialog.reviewResult().addedTracks.size() == 1);
        answerNextMessageBox(QMessageBox::Yes, shown);
        remove->click();
        assert(shown == 2 && dialog.reviewResult().addedTracks.isEmpty());
        std::puts("manual track removal asks: ok");
    }

    void testKeyboardAddsManualTrack()
    {
        QTemporaryDir temp;
        auto request = renderableRequest(temp);
        if (!request)
        {
            std::puts("SKIP keyboard adds a manual track: FFmpeg not found");
            return;
        }
        request->initialFrame = 5;
        cloakframe::VideoReviewDialog dialog(*request);
        dialog.show();
        assert(QTest::qWaitFor(
            [&]
            {
                return dialog.property("lastViewedFrame").toInt() == 5;
            },
            30000));
        auto *canvas = dialog.findChild<QWidget *>("videoCanvas");
        auto *timeline = dialog.findChild<QSlider *>("videoTimeline");
        dialog.findChild<QPushButton *>("addManualTrack")->click();
        assert(QApplication::focusWidget() == canvas);
        QTest::keyClick(canvas, Qt::Key_Right);
        QTest::keyClick(canvas, Qt::Key_Down, Qt::AltModifier);
        assert(timeline->value() == 5);
        QTest::keyClick(canvas, Qt::Key_Return);
        const auto added = dialog.reviewResult().addedTracks;
        assert(added.size() == 1 && added.front().keyframes.size() == 1);
        assert(added.front().keyframes.front().frame == 5);
        assert(added.front().keyframes.front().rect == QRectF(145.2, 102, 36, 39.2));
        assert(dialog.isVisible());

        // Updating a keyframe starts from where the track is, not from the middle.
        dialog.findChild<QPushButton *>("addKeyframe")->click();
        QTest::keyClick(canvas, Qt::Key_Left, Qt::ShiftModifier);
        QTest::keyClick(canvas, Qt::Key_Return);
        const auto updated = dialog.reviewResult().addedTracks;
        assert(updated.size() == 1 && updated.front().keyframes.size() == 1);
        assert(updated.front().keyframes.front().rect == QRectF(142, 102, 36, 39.2));
        std::puts("keyboard adds a manual track: ok");
    }

    void testEncodeAsksToPlaceThePendingBox(
        QApplication &application, const cloakframe::VideoReviewRequest &request)
    {
        int shown = 0;
        cloakframe::VideoReviewDialog dialog(request);
        dialog.show();
        application.processEvents();
        dialog.findChild<QPushButton *>("addManualTrack")->click();
        auto *encode = dialog.findChild<QPushButton *>("encodeVideo");

        answerNextMessageBox(QMessageBox::No, shown);
        encode->click();
        assert(shown == 1 && dialog.isVisible());
        assert(dialog.reviewResult().addedTracks.isEmpty());

        auto *canvas = dialog.findChild<QWidget *>("videoCanvas");
        canvas->activateWindow();
        assert(QTest::qWaitForWindowActive(&dialog));
        answerNextMessageBox(QMessageBox::Yes, shown);
        QTest::keyClick(canvas, Qt::Key_Return, Qt::ControlModifier);
        assert(shown == 2 && !dialog.isVisible());
        const auto result = dialog.reviewResult();
        assert(result.decision == cloakframe::VideoReviewDecision::Encode);
        assert(result.addedTracks.size() == 1);
        assert(result.addedTracks.front().keyframes.size() == 1);
        assert(result.addedTracks.front().keyframes.front().rect == QRectF(142, 102, 36, 36));
        std::puts("encode asks to place the pending box: ok");
    }

    void testExitsKeepTheBatch(
        QApplication &application, const cloakframe::VideoReviewRequest &request)
    {
        using cloakframe::VideoReviewDecision;
        int shown = 0;
        {
            cloakframe::VideoReviewDialog dialog(request);
            dialog.show();
            application.processEvents();
            auto *add = dialog.findChild<QPushButton *>("addManualTrack");
            add->click();
            assert(!add->isEnabled());
            QTest::keyClick(&dialog, Qt::Key_Escape);
            assert(add->isEnabled() && dialog.isVisible() && shown == 0);

            answerNextMessageBox(QMessageBox::No, shown);
            QTest::keyClick(&dialog, Qt::Key_Escape);
            assert(shown == 1 && dialog.isVisible());
            answerNextMessageBox(QMessageBox::Yes, shown);
            dialog.close();
            assert(shown == 2 && !dialog.isVisible());
            assert(dialog.reviewResult().decision == VideoReviewDecision::Skip);
        }
        {
            cloakframe::VideoReviewDialog dialog(request);
            dialog.show();
            application.processEvents();
            dialog.findChild<QPushButton *>("skipVideo")->click();
            assert(shown == 2 && !dialog.isVisible());
            assert(dialog.reviewResult().decision == VideoReviewDecision::Skip);
        }
        {
            cloakframe::VideoReviewDialog dialog(request);
            dialog.show();
            application.processEvents();
            auto *cancelAll = dialog.findChild<QPushButton *>("cancelAll");
            answerNextMessageBox(QMessageBox::No, shown);
            cancelAll->click();
            assert(shown == 3 && dialog.isVisible());
            assert(dialog.reviewResult().decision == VideoReviewDecision::Encode);
            answerNextMessageBox(QMessageBox::Yes, shown);
            cancelAll->click();
            assert(shown == 4 && !dialog.isVisible());
            assert(dialog.reviewResult().decision == VideoReviewDecision::CancelAll);
        }
        std::puts("video review exits keep the batch: ok");
    }
}

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
    request.uncoveredSpans = {{90, 95}, {10, 15}, {10, 20}, {45, 50}, {0, 2}};
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

    const auto pressReturn = [&application](QWidget *target)
    {
        target->setFocus();
        application.processEvents();
        QTest::keyClick(target, Qt::Key_Return);
        application.processEvents();
    };
    list->setCurrentRow(0);
    for (QWidget *target : {static_cast<QWidget *>(&dialog),
             static_cast<QWidget *>(list),
             static_cast<QWidget *>(gaps),
             static_cast<QWidget *>(timeline)})
    {
        pressReturn(target);
        assert(dialog.isVisible() && dialog.result() == QDialog::Rejected);
    }

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
    // No frame of this request can be rendered, so no gap can be acknowledged.
    for (int row = 0; row < gaps->count(); ++row)
    {
        assert(!userCheckable(gaps->item(row)));
    }
    gaps->item(1)->setCheckState(Qt::Checked);
    assert(dialog.reviewResult().acknowledgedGapIndices == QVector<int>{1});
    gaps->setCurrentRow(4);
    assert(dialog.reviewResult().acknowledgedGapIndices == QVector<int>{1});
    list->item(0)->setCheckState(Qt::Unchecked);
    assert(dialog.reviewResult().acknowledgedGapIndices.isEmpty());
    assert(dialog.reviewResult().excludedTrackIds == QVector<int>{7});
    list->item(0)->setCheckState(Qt::Checked);
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());
    gaps->item(1)->setCheckState(Qt::Checked);
    buttonWithText(dialog, "Exclude all")->click();
    assert(dialog.reviewResult().acknowledgedGapIndices.isEmpty());
    gaps->item(1)->setCheckState(Qt::Checked);
    buttonWithText(dialog, "Exclude all")->click();
    assert(dialog.reviewResult().acknowledgedGapIndices == QVector<int>{1});
    buttonWithText(dialog, "Include all")->click();
    assert(dialog.reviewResult().acknowledgedGapIndices.isEmpty());
    assert(dialog.reviewResult().excludedTrackIds.isEmpty());
    testGapChecksFollowWhatWasShown(application);
    testManualTrackRemovalAsks();
    testKeyboardAddsManualTrack();
    testEncodeAsksToPlaceThePendingBox(application, request);
    testExitsKeepTheBatch(application, request);
    auto jumpRequest = request;
    jumpRequest.initialFrame = 45;
    cloakframe::VideoReviewDialog jumped(jumpRequest);
    assert(jumped.findChild<QSlider *>("videoTimeline")->value() == 45);

    dialog.activateWindow();
    list->setFocus();
    application.processEvents();
    QTest::keyClick(list, Qt::Key_Return, Qt::ControlModifier);
    application.processEvents();
    assert(!dialog.isVisible() && dialog.result() == QDialog::Accepted);
    assert(dialog.reviewResult().decision == cloakframe::VideoReviewDecision::Encode);
    return 0;
}
