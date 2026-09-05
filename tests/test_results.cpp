#include "cloakframe/ResultsDialog.hpp"

#include <QApplication>
#include <QComboBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTranslator>

#include <cassert>

int main(int argc, char **argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QApplication application(argc, argv);
    using cloakframe::FileResultStatus;
    const QVector<cloakframe::FileResult> results{
        {"/photos/z.jpg", "/results/z.jpg", FileResultStatus::Saved, {"Saved z"}},
        {"/photos/a.jpg", "/results/a.jpg", FileResultStatus::NeedsReview, {"No regions redacted"}},
        {"/photos/b.jpg", {}, FileResultStatus::Failed, {"Existing output would be overwritten"}},
        {"/photos/c.jpg", {}, FileResultStatus::Skipped, {"Skipped without saving"}},
        {"/photos/d.jpg", {}, FileResultStatus::Cancelled, {"Cancelled"}},
        {"/photos/private", {}, FileResultStatus::UnreadableInput, {"Permission denied"}},
    };
    cloakframe::ResultsDialog dialog(results);
    auto *table = dialog.findChild<QTableWidget *>();
    auto *filter = dialog.findChild<QComboBox *>();
    auto *details = dialog.findChild<QPlainTextEdit *>();
    QPushButton *openOutput = nullptr;
    for (auto *button : dialog.findChildren<QPushButton *>())
    {
        if (button->text() == "Open output folder")
        {
            openOutput = button;
        }
    }
    assert(table && filter && details && openOutput);
    assert(table->rowCount() == 6);
    assert(table->item(0, 0)->text() == "/photos/a.jpg");
    assert(details->toPlainText() == "No regions redacted");
    assert(openOutput->isEnabled());
    filter->setCurrentIndex(2);
    int visible = 0;
    for (int row = 0; row < table->rowCount(); ++row)
    {
        visible += !table->isRowHidden(row);
    }
    assert(visible == 2);
    assert(details->toPlainText() == "Existing output would be overwritten");
    assert(!openOutput->isEnabled());
    table->sortItems(0, Qt::DescendingOrder);
    visible = 0;
    for (int row = 0; row < table->rowCount(); ++row)
    {
        if (!table->isRowHidden(row))
        {
            const QString status = table->item(row, 1)->text();
            assert(status == "Failed" || status == "Unreadable input");
            ++visible;
        }
    }
    assert(visible == 2);
    filter->setCurrentIndex(1);
    visible = 0;
    for (int row = 0; row < table->rowCount(); ++row)
    {
        visible += !table->isRowHidden(row);
    }
    assert(visible == 5);
    filter->setCurrentIndex(0);
    table->sortItems(0, Qt::DescendingOrder);
    table->selectRow(0);
    assert(details->toPlainText() == "Saved z");
    assert(openOutput->isEnabled());

    cloakframe::ResultsDialog savedOnly({results.front()});
    savedOnly.findChild<QComboBox *>()->setCurrentIndex(2);
    assert(savedOnly.findChild<QPlainTextEdit *>()->toPlainText().isEmpty());
    for (auto *button : savedOnly.findChildren<QPushButton *>())
    {
        if (button->text().startsWith("Open "))
        {
            assert(!button->isEnabled());
        }
    }
    cloakframe::ResultsDialog empty({});
    assert(empty.findChild<QTableWidget *>()->rowCount() == 0);
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
        cloakframe::ResultsDialog preview(results);
        preview.show();
        application.processEvents();
        assert(preview.grab().save(screenshot));
    }
    return 0;
}
