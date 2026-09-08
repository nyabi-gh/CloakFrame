#include "cloakframe/ResultsDialog.hpp"

#include <QComboBox>
#include <QDesktopServices>
#include <QDialogButtonBox>
#include <QFileInfo>
#include <QHeaderView>
#include <QLabel>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QUrl>
#include <QVBoxLayout>

#include <algorithm>
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

        issueFilter_ = new QComboBox(this);
        issueFilter_->setObjectName("issueFilter");
        issueFilter_->setAccessibleName(tr("All issue types"));
        issueFilter_->addItem(tr("All issue types"), -1);
        for (const auto &[kind, label] : std::initializer_list<std::pair<FileIssueKind, QString>>{
                 {FileIssueKind::OmittedRegions, tr("Omitted regions")},
                 {FileIssueKind::TrackingGap, tr("Tracking gaps")},
                 {FileIssueKind::DroppedTracks, tr("Dropped tracks")},
                 {FileIssueKind::ExcludedTracks, tr("Excluded tracks")},
                 {FileIssueKind::ScanFailure, tr("Scan failures")},
                 {FileIssueKind::MetadataWarning, tr("Metadata warnings")},
                 {FileIssueKind::OutputConflict, tr("Output conflicts")},
                 {FileIssueKind::ProcessingFailure, tr("Processing failures")},
                 {FileIssueKind::UnredactedOutput, tr("Unredacted outputs")}})
            issueFilter_->addItem(label, static_cast<int>(kind));
        layout->addWidget(issueFilter_);

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

        issues_ = new QListWidget(this);
        issues_->setObjectName("resultIssues");
        issues_->setAccessibleName(tr("Issues in selected file"));
        issues_->setMaximumHeight(110);
        layout->addWidget(issues_);
        openFile_ = new QPushButton(tr("Open input file"), this);
        retry_ = new QPushButton(tr("Review / retry selected input"), this);
        retry_->setObjectName("retryInput");
        retry_->setToolTip(tr("Reprocess this input with current settings in a new output folder. "
                              "Select a tracking gap to start video review at that frame. Previous "
                              "edits are not retained."));
        layout->addWidget(openFile_);
        layout->addWidget(retry_);
        connect(retry_,
            &QPushButton::clicked,
            this,
            [this]
            {
                const auto *result = selectedResult();
                if (!result)
                    return;
                const int frame = issues_->currentItem()
                                      ? issues_->currentItem()->data(Qt::UserRole).toInt()
                                      : -1;
                emit retryRequested(result->sourcePath, frame);
                accept();
            });
        connect(openFile_,
            &QPushButton::clicked,
            this,
            [this]
            {
                const auto *result = selectedResult();
                if (result && !QDesktopServices::openUrl(QUrl::fromLocalFile(result->sourcePath)))
                    QMessageBox::warning(this,
                        tr("Cannot open file"),
                        tr("Could not open: %1").arg(result->sourcePath));
            });
        connect(issueFilter_, &QComboBox::currentIndexChanged, this, &ResultsDialog::filterRows);
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
            const auto &result = results_[table_->item(row, 0)->data(Qt::UserRole).toInt()];
            const auto status = result.status;
            const bool statusMatches =
                filter_->currentIndex() == 0
                || (filter_->currentIndex() == 1 && status != FileResultStatus::Saved)
                || (filter_->currentIndex() == 2
                    && (status == FileResultStatus::Failed
                        || status == FileResultStatus::UnreadableInput));
            const int kind = issueFilter_->currentData().toInt();
            const bool visible = statusMatches
                                 && (kind < 0
                                     || std::any_of(
                                         result.issues.begin(),
                                         result.issues.end(),
                                         [kind](const FileIssue &issue)
                                         {
                                             return static_cast<int>(issue.kind) == kind;
                                         }));
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
        issues_->clear();
        if (result)
        {
            for (const auto &issue : result->issues)
            {
                const int index = issueFilter_->findData(static_cast<int>(issue.kind));
                QString text =
                    issueFilter_->itemText(index) + QStringLiteral(": %1").arg(issue.count);
                if (issue.kind == FileIssueKind::TrackingGap)
                    text =
                        tr("Track %1 · Frames %2–%3 · %4")
                            .arg(issue.trackId)
                            .arg(issue.firstFrame + 1)
                            .arg(issue.lastFrame + 1)
                            .arg(issue.acknowledged ? tr("Reviewed by user (coverage not verified)")
                                                    : tr("Pending review"));
                auto *item = new QListWidgetItem(text, issues_);
                item->setData(Qt::UserRole, issue.firstFrame);
            }
        }
        issues_->setVisible(issues_->count() > 0);
        if (issues_->count() > 0)
            issues_->setCurrentRow(0);
        retry_->setEnabled(result != nullptr);
        openFile_->setEnabled(result != nullptr);
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
