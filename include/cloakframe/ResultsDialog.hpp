#pragma once

#include "cloakframe/FileResult.hpp"

#include <QDialog>
#include <QVector>

class QComboBox;
class QPlainTextEdit;
class QPushButton;
class QTableWidget;

namespace cloakframe
{
    class ResultsDialog final : public QDialog
    {
        Q_OBJECT

    public:
        explicit ResultsDialog(QVector<FileResult> results, QWidget *parent = nullptr);

    private:
        void filterRows();
        void updateSelection();
        void openFolder(bool output);
        [[nodiscard]] const FileResult *selectedResult() const;
        [[nodiscard]] static QString statusText(FileResultStatus status);

        QVector<FileResult> results_;
        QComboBox *filter_ = nullptr;
        QTableWidget *table_ = nullptr;
        QPlainTextEdit *details_ = nullptr;
        QPushButton *openSource_ = nullptr;
        QPushButton *openOutput_ = nullptr;
    };
}
