#include "cloakframe/ResultsDialog.hpp"

#include <QComboBox>
#include <QDesktopServices>
#include <QDialogButtonBox>
#include <QFileInfo>
#include <QHeaderView>
#include <QLabel>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QUrl>
#include <QVBoxLayout>

#include <utility>

namespace cloakframe
{
    ResultsDialog::ResultsDialog(QVector<FileResult> results, QWidget *parent)
        : QDialog(parent)
        , results_(std::move(results))
    {
        setWindowTitle(tr("File results"));
        resize(900, 640);
        auto *layout = new QVBoxLayout(this);
        auto *hint = new QLabel(tr("Results from the last run. Only reported files and input "
                                   "errors are listed; files that never started are not shown."),
            this);
        hint->setWordWrap(true);
        layout->addWidget(hint);
        filter_ = new QComboBox(this);
        filter_->setAccessibleName(tr("Filter results"));
        filter_->addItems({tr("All results"), tr("Needs attention"), tr("Failures")});
        layout->addWidget(filter_);

        table_ = new QTableWidget(static_cast<int>(results_.size()), 3, this);
        table_->setAccessibleName(tr("File results"));
        table_->setHorizontalHeaderLabels({tr("Input"), tr("Status"), tr("Output")});
        table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
        table_->setSelectionBehavior(QAbstractItemView::SelectRows);
        table_->setSelectionMode(QAbstractItemView::SingleSelection);
        table_->verticalHeader()->hide();
        table_->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
        table_->horizontalHeader()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
        table_->horizontalHeader()->setSectionResizeMode(2, QHeaderView::Stretch);
        for (int row = 0; row < table_->rowCount(); ++row)
        {
            const auto &result = results_[row];
            auto *input = new QTableWidgetItem(result.sourcePath);
            input->setData(Qt::UserRole, row);
            input->setToolTip(result.sourcePath);
            table_->setItem(row, 0, input);
            table_->setItem(row, 1, new QTableWidgetItem(statusText(result.status)));
            auto *output = new QTableWidgetItem(
                result.outputPath.isEmpty() ? tr("Not saved") : result.outputPath);
            output->setToolTip(result.outputPath);
            table_->setItem(row, 2, output);
        }
        table_->setSortingEnabled(true);
        table_->sortItems(0, Qt::AscendingOrder);
        layout->addWidget(table_, 3);
        details_ = new QPlainTextEdit(this);
        details_->setReadOnly(true);
        details_->setAccessibleName(tr("Result details"));
        details_->setPlaceholderText(tr("Select a result to see its details."));
        layout->addWidget(details_, 1);

        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
        openSource_ = buttons->addButton(tr("Open input folder"), QDialogButtonBox::ActionRole);
        openOutput_ = buttons->addButton(tr("Open output folder"), QDialogButtonBox::ActionRole);
        layout->addWidget(buttons);
        connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
        connect(filter_, &QComboBox::currentIndexChanged, this, &ResultsDialog::filterRows);
        connect(
            table_->model(), &QAbstractItemModel::layoutChanged, this, &ResultsDialog::filterRows);
        connect(table_, &QTableWidget::itemSelectionChanged, this, &ResultsDialog::updateSelection);
        connect(openSource_,
            &QPushButton::clicked,
            this,
            [this]
            {
                openFolder(false);
            });
        connect(openOutput_,
            &QPushButton::clicked,
            this,
            [this]
            {
                openFolder(true);
            });
        filterRows();
    }

    QString ResultsDialog::statusText(FileResultStatus status)
    {
        switch (status)
        {
        case FileResultStatus::Saved:
            return tr("Saved");
        case FileResultStatus::NeedsReview:
            return tr("Review required");
        case FileResultStatus::Skipped:
            return tr("Skipped without saving");
        case FileResultStatus::Failed:
            return tr("Failed");
        case FileResultStatus::Cancelled:
            return tr("Cancelled");
        case FileResultStatus::UnreadableInput:
            return tr("Unreadable input");
        }
        return {};
    }

    const FileResult *ResultsDialog::selectedResult() const
    {
        const int row = table_->currentRow();
        if (row < 0 || table_->isRowHidden(row) || table_->selectedItems().isEmpty())
        {
            return nullptr;
        }
        const int index = table_->item(row, 0)->data(Qt::UserRole).toInt();
        return &results_[index];
    }

    void ResultsDialog::filterRows()
    {
        table_->clearSelection();
        int firstVisible = -1;
        for (int row = 0; row < table_->rowCount(); ++row)
        {
            const auto status = results_[table_->item(row, 0)->data(Qt::UserRole).toInt()].status;
            const bool visible =
                filter_->currentIndex() == 0
                || (filter_->currentIndex() == 1 && status != FileResultStatus::Saved)
                || (filter_->currentIndex() == 2
                    && (status == FileResultStatus::Failed
                        || status == FileResultStatus::UnreadableInput));
            table_->setRowHidden(row, !visible);
            if (visible && firstVisible < 0)
            {
                firstVisible = row;
            }
        }
        if (firstVisible >= 0)
        {
            table_->selectRow(firstVisible);
        }
        updateSelection();
    }

    void ResultsDialog::updateSelection()
    {
        const auto *result = selectedResult();
        details_->setPlainText(result ? result->messages.join('\n') : QString());
        openSource_->setEnabled(result != nullptr);
        openOutput_->setEnabled(result != nullptr && !result->outputPath.isEmpty());
    }

    void ResultsDialog::openFolder(bool output)
    {
        const auto *result = selectedResult();
        if (result == nullptr || (output && result->outputPath.isEmpty()))
        {
            return;
        }
        const QFileInfo info(output ? result->outputPath : result->sourcePath);
        const QString folder =
            !output && info.isDir() ? info.absoluteFilePath() : info.absolutePath();
        if (!QFileInfo(folder).isDir() || !QDesktopServices::openUrl(QUrl::fromLocalFile(folder)))
        {
            QMessageBox::warning(
                this, tr("Cannot open folder"), tr("Could not open: %1").arg(folder));
        }
    }
}
