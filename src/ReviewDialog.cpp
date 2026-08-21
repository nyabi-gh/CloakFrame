#include "cloakframe/ReviewDialog.hpp"

#include <QFrame>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QKeySequence>
#include <QLabel>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QPushButton>
#include <QShortcut>
#include <QSizePolicy>
#include <QVBoxLayout>
#include <QWheelEvent>
#include <QWidget>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <functional>
#include <vector>

namespace cloakframe
{
    namespace
    {
        struct Box
        {
            QRectF rect;
            bool detected;
            bool included;
        };

        constexpr int kMinBoxPixels = 6;
    }

    class ReviewCanvas final : public QWidget
    {
    public:
        ReviewCanvas(
            QImage image, const QVector<QRectF> &detected, ReviewPreviewSpec spec, QWidget *parent)
            : QWidget(parent)
            , image_(std::move(image))
            , spec_(std::move(spec))
        {
            setMouseTracking(true);
            setFocusPolicy(Qt::StrongFocus);
            const QRectF imageBounds(QPointF(0, 0), QSizeF(image_.size()));
            boxes_.reserve(detected.size());
            for (const auto &rect : detected)
            {
                const QRectF clamped = rect.intersected(imageBounds);
                if (clamped.width() >= 1.0 && clamped.height() >= 1.0)
                {
                    boxes_.push_back(Box{clamped, true, true});
                }
            }
            setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
        }

        [[nodiscard]] QVector<QRectF> finalBoxes() const
        {
            QVector<QRectF> result;
            result.reserve(boxes_.size());
            const QRectF imageBounds(QPointF(0, 0), QSizeF(image_.size()));
            for (const auto &box : boxes_)
            {
                if (!box.detected || box.included)
                {

                    const QRectF clamped = box.rect.intersected(imageBounds);
                    if (clamped.width() >= 1.0 && clamped.height() >= 1.0)
                    {
                        result.push_back(clamped);
                    }
                }
            }
            return result;
        }

        [[nodiscard]] qsizetype detectedCount() const
        {
            return std::count_if(boxes_.begin(),
                boxes_.end(),
                [](const Box &box)
                {
                    return box.detected;
                });
        }

        [[nodiscard]] bool canUndo() const
        {
            return !undoStack_.empty();
        }
        [[nodiscard]] bool canRedo() const
        {
            return !redoStack_.empty();
        }

        void undo()
        {
            if (undoStack_.empty())
            {
                return;
            }
            redoStack_.push_back(boxes_);
            boxes_ = undoStack_.back();
            undoStack_.pop_back();
            hoveredIndex_ = -1;
            clampFocus();
            update();
            emitHistoryChanged();
        }

        void redo()
        {
            if (redoStack_.empty())
            {
                return;
            }
            undoStack_.push_back(boxes_);
            boxes_ = redoStack_.back();
            redoStack_.pop_back();
            hoveredIndex_ = -1;
            clampFocus();
            update();
            emitHistoryChanged();
        }

        void setHistoryChangedCallback(std::function<void()> cb)
        {
            historyChanged_ = std::move(cb);
        }

        [[nodiscard]] QSize sizeHint() const override
        {
            return fitSize(image_.size());
        }

        [[nodiscard]] QSize minimumSizeHint() const override
        {
            return QSize(420, 320);
        }

        void resetView()
        {
            zoom_ = 1.0;
            pan_ = QPointF();
            update();
        }

    protected:
        void paintEvent(QPaintEvent *) override
        {
            QPainter painter(this);
            painter.setRenderHint(QPainter::Antialiasing, true);
            painter.fillRect(rect(), QColor("#111827"));

            if (image_.isNull())
            {
                return;
            }

            const QRectF target = imageTargetRect();

            if (peeking_ && !peekImage_.isNull())
            {
                painter.drawImage(target, peekImage_);
                return;
            }

            painter.drawImage(target, image_);

            for (int i = 0; i < boxes_.size(); ++i)
            {
                const auto &box = boxes_[i];
                const QRectF screen = imageToScreen(box.rect);

                QColor stroke;
                QColor fill;
                if (!box.detected)
                {
                    stroke = QColor("#3B82F6");
                    fill = QColor(59, 130, 246, 60);
                }
                else if (box.included)
                {
                    stroke = QColor("#F59E0B");
                    fill = QColor(245, 158, 11, 60);
                }
                else
                {
                    stroke = QColor("#9CA3AF");
                    fill = QColor(156, 163, 175, 30);
                }

                painter.setPen(QPen(stroke, 2));
                painter.setBrush(fill);
                painter.drawRect(screen);

                if (box.detected && !box.included)
                {
                    painter.setPen(QPen(QColor("#EF4444"), 2));
                    painter.drawLine(screen.topLeft(), screen.bottomRight());
                    painter.drawLine(screen.topRight(), screen.bottomLeft());
                }

                if (i == hoveredIndex_)
                {
                    painter.setPen(QPen(QColor("#FFFFFF"), 1, Qt::DashLine));
                    painter.setBrush(Qt::NoBrush);
                    painter.drawRect(screen.adjusted(-2, -2, 2, 2));
                }

                if (i == focusedIndex_)
                {
                    painter.setPen(QPen(QColor("#FFFFFF"), 2));
                    painter.setBrush(Qt::NoBrush);
                    painter.drawRect(screen.adjusted(-4, -4, 4, 4));
                }
            }

            if (drawing_)
            {
                painter.setPen(QPen(QColor("#3B82F6"), 2, Qt::DashLine));
                painter.setBrush(QColor(59, 130, 246, 40));
                painter.drawRect(QRectF(dragStart_, dragCurrent_).normalized());
            }
        }

        void wheelEvent(QWheelEvent *event) override
        {
            if (image_.isNull())
            {
                return;
            }
            const double steps = event->angleDelta().y() / 120.0;
            if (steps == 0.0)
            {
                return;
            }
            const qreal factor = std::pow(1.25, steps);
            const qreal newZoom = std::clamp(zoom_ * factor, 1.0, 8.0);
            if (newZoom == zoom_)
            {
                event->accept();
                return;
            }
            const QPointF pos = event->position();
            const QRectF before = imageTargetRect();
            const qreal relX = before.width() > 0 ? (pos.x() - before.x()) / before.width() : 0.5;
            const qreal relY = before.height() > 0 ? (pos.y() - before.y()) / before.height() : 0.5;
            zoom_ = newZoom;
            if (zoom_ == 1.0)
            {
                pan_ = QPointF();
            }
            else
            {
                const QSizeF scaled = QSizeF(fitSize(image_.size())) * zoom_;
                const qreal centeredX = (width() - scaled.width()) / 2.0;
                const qreal centeredY = (height() - scaled.height()) / 2.0;
                pan_ = QPointF(pos.x() - relX * scaled.width() - centeredX,
                    pos.y() - relY * scaled.height() - centeredY);
                clampPan();
            }
            update();
            event->accept();
        }

        void mousePressEvent(QMouseEvent *event) override
        {
            if (event->button() == Qt::RightButton || event->button() == Qt::MiddleButton)
            {
                if (zoom_ > 1.0)
                {
                    panning_ = true;
                    panStart_ = event->position();
                    panOrigin_ = pan_;
                    setCursor(Qt::ClosedHandCursor);
                }
                return;
            }
            if (event->button() != Qt::LeftButton || image_.isNull())
            {
                return;
            }
            const QPointF pos = event->position();
            const int hit = hitTest(pos);
            if (hit >= 0)
            {
                focusedIndex_ = hit;
                activateBox(hit);
                return;
            }
            if (!imageTargetRect().contains(pos))
            {
                return;
            }
            drawing_ = true;
            dragStart_ = pos;
            dragCurrent_ = pos;
            update();
        }

        void mouseMoveEvent(QMouseEvent *event) override
        {
            const QPointF pos = event->position();
            if (panning_)
            {
                pan_ = panOrigin_ + (pos - panStart_);
                clampPan();
                update();
                return;
            }
            if (drawing_)
            {
                dragCurrent_ = pos;
                update();
                return;
            }
            const int hit = hitTest(pos);
            if (hit != hoveredIndex_)
            {
                hoveredIndex_ = hit;
                setCursor(hit >= 0 ? Qt::PointingHandCursor : Qt::CrossCursor);
                update();
            }
        }

        void mouseReleaseEvent(QMouseEvent *event) override
        {
            if (panning_
                && (event->button() == Qt::RightButton || event->button() == Qt::MiddleButton))
            {
                panning_ = false;
                setCursor(
                    hitTest(event->position()) >= 0 ? Qt::PointingHandCursor : Qt::CrossCursor);
                return;
            }
            if (!drawing_ || event->button() != Qt::LeftButton)
            {
                return;
            }
            drawing_ = false;
            const QRectF screenRect = QRectF(dragStart_, dragCurrent_).normalized();
            if (screenRect.width() >= kMinBoxPixels && screenRect.height() >= kMinBoxPixels)
            {
                const QRectF imageRect =
                    screenToImage(screenRect)
                        .intersected(QRectF(QPointF(0, 0), QSizeF(image_.size())));
                if (imageRect.width() >= 1.0 && imageRect.height() >= 1.0)
                {
                    pushUndoSnapshot();
                    boxes_.push_back(Box{imageRect, false, true});
                }
            }
            update();
        }

        void keyPressEvent(QKeyEvent *event) override
        {
            switch (event->key())
            {
            case Qt::Key_Space:
                if (!event->isAutoRepeat())
                {
                    peeking_ = true;
                    peekImage_ = renderPeek();
                    update();
                }
                return;
            case Qt::Key_0:
            case Qt::Key_F:
                resetView();
                return;
            case Qt::Key_Left:
            case Qt::Key_Up:
                moveFocus(-1);
                return;
            case Qt::Key_Right:
            case Qt::Key_Down:
                moveFocus(1);
                return;
            case Qt::Key_Return:
            case Qt::Key_Enter:
                if (focusedIndex_ >= 0)
                {
                    activateBox(focusedIndex_);
                    return;
                }
                break;
            case Qt::Key_Delete:
            case Qt::Key_Backspace:
                if (focusedIndex_ >= 0)
                {
                    excludeBox(focusedIndex_);
                    return;
                }
                break;
            default:
                break;
            }
            QWidget::keyPressEvent(event);
        }

        void keyReleaseEvent(QKeyEvent *event) override
        {
            if (event->key() == Qt::Key_Space && !event->isAutoRepeat())
            {
                peeking_ = false;
                peekImage_ = QImage();
                update();
                return;
            }
            QWidget::keyReleaseEvent(event);
        }

    private:
        [[nodiscard]] QRectF imageTargetRect() const
        {
            if (image_.isNull())
            {
                return {};
            }
            const QSizeF scaled = QSizeF(fitSize(image_.size())) * zoom_;
            const double x = (width() - scaled.width()) / 2.0 + pan_.x();
            const double y = (height() - scaled.height()) / 2.0 + pan_.y();
            return QRectF(QPointF(x, y), scaled);
        }

        void clampPan()
        {
            const QSizeF scaled = QSizeF(fitSize(image_.size())) * zoom_;
            const qreal maxX = std::max(0.0, (scaled.width() - width()) / 2.0 + 60.0);
            const qreal maxY = std::max(0.0, (scaled.height() - height()) / 2.0 + 60.0);
            pan_.setX(std::clamp(pan_.x(), -maxX, maxX));
            pan_.setY(std::clamp(pan_.y(), -maxY, maxY));
        }

        void clampFocus()
        {
            const auto boxCount = static_cast<int>(boxes_.size());
            if (focusedIndex_ >= boxCount)
            {
                focusedIndex_ = boxCount - 1;
            }
        }

        void moveFocus(int delta)
        {
            if (boxes_.isEmpty())
            {
                focusedIndex_ = -1;
                update();
                return;
            }
            const auto boxCount = static_cast<int>(boxes_.size());
            if (focusedIndex_ < 0)
            {
                focusedIndex_ = delta > 0 ? 0 : boxCount - 1;
            }
            else
            {
                focusedIndex_ = (focusedIndex_ + delta + boxCount) % boxCount;
            }
            update();
        }

        void activateBox(int index)
        {
            if (index < 0 || index >= boxes_.size())
            {
                return;
            }
            pushUndoSnapshot();
            auto &box = boxes_[index];
            if (box.detected)
            {
                box.included = !box.included;
            }
            else
            {
                boxes_.remove(index);
                clampFocus();
            }
            hoveredIndex_ = -1;
            update();
        }

        void excludeBox(int index)
        {
            if (index < 0 || index >= boxes_.size())
            {
                return;
            }
            if (boxes_[index].detected && !boxes_[index].included)
            {
                return;
            }
            pushUndoSnapshot();
            auto &box = boxes_[index];
            if (box.detected)
            {
                box.included = false;
            }
            else
            {
                boxes_.remove(index);
                clampFocus();
            }
            hoveredIndex_ = -1;
            update();
        }

        [[nodiscard]] QImage renderPeek() const
        {
            QImage base = image_.convertToFormat(QImage::Format_BGR888);
            if (base.isNull())
            {
                return {};
            }
            cv::Mat mat(base.height(),
                base.width(),
                CV_8UC3,
                base.bits(),
                static_cast<size_t>(base.bytesPerLine()));
            FaceDetections detections;
            for (const auto &rect : finalBoxes())
            {
                FaceDetection det;
                det.box = cv::Rect2f(static_cast<float>(rect.x()),
                    static_cast<float>(rect.y()),
                    static_cast<float>(rect.width()),
                    static_cast<float>(rect.height()));
                det.score = 1.0F;
                detections.push_back(det);
            }
            const int blockSize =
                std::max(2, static_cast<int>(std::round(spec_.blockSize * spec_.previewScale)));
            applyAnonymization(mat,
                detections,
                spec_.method,
                blockSize,
                spec_.padding,
                spec_.shape,
                spec_.softEdges,
                spec_.customImage);
            return base;
        }

        [[nodiscard]] QSize fitSize(QSize source) const
        {
            if (source.isEmpty())
            {
                return QSize(420, 320);
            }
            const double aspect = static_cast<double>(source.width()) / source.height();
            const int w = std::max(1, width());
            const int h = std::max(1, height());
            double fitW = w;
            double fitH = fitW / aspect;
            if (fitH > h)
            {
                fitH = h;
                fitW = fitH * aspect;
            }
            return QSize(static_cast<int>(fitW), static_cast<int>(fitH));
        }

        [[nodiscard]] QRectF imageToScreen(const QRectF &rect) const
        {
            const QRectF target = imageTargetRect();
            const double sx = target.width() / image_.width();
            const double sy = target.height() / image_.height();
            return QRectF(target.x() + rect.x() * sx,
                target.y() + rect.y() * sy,
                rect.width() * sx,
                rect.height() * sy);
        }

        [[nodiscard]] QRectF screenToImage(const QRectF &rect) const
        {
            const QRectF target = imageTargetRect();
            if (target.width() <= 0 || target.height() <= 0)
            {
                return {};
            }
            const double sx = image_.width() / target.width();
            const double sy = image_.height() / target.height();
            return QRectF((rect.x() - target.x()) * sx,
                (rect.y() - target.y()) * sy,
                rect.width() * sx,
                rect.height() * sy);
        }

        [[nodiscard]] int hitTest(QPointF pos) const
        {
            for (int i = static_cast<int>(boxes_.size()) - 1; i >= 0; --i)
            {
                if (imageToScreen(boxes_[i].rect).contains(pos))
                {
                    return i;
                }
            }
            return -1;
        }

        void pushUndoSnapshot()
        {
            undoStack_.push_back(boxes_);
            redoStack_.clear();
            if (undoStack_.size() > kMaxUndo)
            {
                undoStack_.erase(undoStack_.begin());
            }
            emitHistoryChanged();
        }

        void emitHistoryChanged() const
        {
            if (historyChanged_)
            {
                historyChanged_();
            }
        }

        static constexpr std::size_t kMaxUndo = 64;

        QImage image_;
        ReviewPreviewSpec spec_;
        QVector<Box> boxes_;
        int hoveredIndex_ = -1;
        int focusedIndex_ = -1;
        bool drawing_ = false;
        QPointF dragStart_;
        QPointF dragCurrent_;
        qreal zoom_ = 1.0;
        QPointF pan_;
        bool panning_ = false;
        QPointF panStart_;
        QPointF panOrigin_;
        bool peeking_ = false;
        QImage peekImage_;
        std::vector<QVector<Box>> undoStack_;
        std::vector<QVector<Box>> redoStack_;
        std::function<void()> historyChanged_;
    };

    ReviewDialog::ReviewDialog(const QImage &image,
        const QString &sourceName,
        const QVector<QRectF> &detected,
        int currentIndex,
        int total,
        bool preserveMetadata,
        const ReviewPreviewSpec &previewSpec,
        QWidget *parent)
        : QDialog(parent)
    {
        setWindowTitle(tr("Review — %1").arg(sourceName));
        setModal(true);
        resize(960, 720);

        auto *root = new QVBoxLayout(this);
        root->setContentsMargins(18, 18, 18, 18);
        root->setSpacing(12);

        auto *header = new QLabel(
            QString("<b>%1</b> &nbsp;·&nbsp; <span style='color:%2'>%3 / %4</span>")
                .arg(sourceName.toHtmlEscaped(), palette().placeholderText().color().name())
                .arg(currentIndex)
                .arg(total),
            this);
        header->setTextFormat(Qt::RichText);
        root->addWidget(header);

        canvas_ = new ReviewCanvas(image, detected, previewSpec, this);
        canvas_->setStyleSheet("background-color: #111827; border-radius: 8px;");
        canvas_->setAccessibleName(tr("Review image"));
        canvas_->setFocus(Qt::OtherFocusReason);
        root->addWidget(canvas_, 1);

        hintLabel_ =
            new QLabel(tr("Click or Return toggles a box · Drag an empty area to add · "
                          "Arrow keys move the selection · Hold Space to preview the result · "
                          "Scroll to zoom, right-drag to pan, 0 resets · %1 / %2 to undo/redo · "
                          "Esc skips this image without saving")
                           .arg(QKeySequence(QKeySequence::Undo).toString(QKeySequence::NativeText),
                               QKeySequence(QKeySequence::Redo).toString(QKeySequence::NativeText)),
                this);
        hintLabel_->setProperty("role", "sectionHint");
        hintLabel_->setWordWrap(true);
        root->addWidget(hintLabel_);

        auto *buttonRow = new QHBoxLayout();
        buttonRow->setSpacing(8);

        auto *cancelAll = new QPushButton(tr("Cancel All"), this);
        cancelAll->setCursor(Qt::PointingHandCursor);

        auto *undoButton = new QPushButton(tr("Undo"), this);
        undoButton->setCursor(Qt::PointingHandCursor);
        undoButton->setEnabled(false);

        auto *redoButton = new QPushButton(tr("Redo"), this);
        redoButton->setCursor(Qt::PointingHandCursor);
        redoButton->setEnabled(false);

        auto *doNotSave = new QPushButton(tr("Do Not Save"), this);
        doNotSave->setCursor(Qt::PointingHandCursor);

        auto *copyOriginal = new QPushButton(tr("Copy Original"), this);
        copyOriginal->setObjectName("dangerButton");
        copyOriginal->setCursor(Qt::PointingHandCursor);
        copyOriginal->setToolTip(tr("Saves the image without anonymizing it."));

        auto *save = new QPushButton(tr("Save && Next"), this);
        save->setObjectName("primaryButton");
        save->setCursor(Qt::PointingHandCursor);
        save->setDefault(true);

        buttonRow->addWidget(cancelAll);
        buttonRow->addWidget(undoButton);
        buttonRow->addWidget(redoButton);
        buttonRow->addStretch(1);
        buttonRow->addWidget(doNotSave);
        buttonRow->addWidget(copyOriginal);
        buttonRow->addWidget(save);
        root->addLayout(buttonRow);

        connect(cancelAll,
            &QPushButton::clicked,
            this,
            [this, currentIndex, total]
            {
                const int remaining = total - currentIndex + 1;
                const auto answer = QMessageBox::question(this,
                    tr("Cancel All?"),
                    tr("Stop reviewing and cancel the remaining %n image(s)?\n\n"
                       "Images already saved are kept.",
                        nullptr,
                        remaining),
                    QMessageBox::Yes | QMessageBox::No,
                    QMessageBox::No);
                if (answer != QMessageBox::Yes)
                {
                    return;
                }
                decision_ = ReviewDecision::CancelAll;
                QDialog::reject();
            });
        connect(undoButton,
            &QPushButton::clicked,
            this,
            [this]
            {
                canvas_->undo();
            });
        connect(redoButton,
            &QPushButton::clicked,
            this,
            [this]
            {
                canvas_->redo();
            });
        connect(doNotSave,
            &QPushButton::clicked,
            this,
            [this]
            {
                decision_ = ReviewDecision::DoNotSave;
                accept();
            });
        connect(copyOriginal,
            &QPushButton::clicked,
            this,
            [this, preserveMetadata]
            {
                const QString detail =
                    preserveMetadata
                        ? tr("The unredacted original will be saved to the output folder, "
                             "including its original metadata (EXIF, GPS, timestamps).")
                        : tr("The unredacted original will be saved to the output folder "
                             "(re-encoded without metadata).");
                const auto answer = QMessageBox::warning(this,
                    tr("Copy Original?"),
                    tr("This image will not be anonymized.\n\n%1\n\nContinue?").arg(detail),
                    QMessageBox::Yes | QMessageBox::No,
                    QMessageBox::No);
                if (answer != QMessageBox::Yes)
                {
                    return;
                }
                decision_ = ReviewDecision::CopyOriginal;
                accept();
            });
        connect(save,
            &QPushButton::clicked,
            this,
            [this]
            {
                if (reviewClearedEveryDetection(
                        canvas_->detectedCount(), canvas_->finalBoxes().size()))
                {
                    const auto answer = QMessageBox::warning(this,
                        tr("Save with nothing covered?"),
                        tr("Every region the detector found has been excluded and none were "
                           "added in their place, so this image will be saved with nothing "
                           "covered.\n\nContinue?"),
                        QMessageBox::Yes | QMessageBox::No,
                        QMessageBox::No);
                    if (answer != QMessageBox::Yes)
                    {
                        return;
                    }
                }
                decision_ = ReviewDecision::Save;
                accept();
            });

        canvas_->setHistoryChangedCallback(
            [this, undoButton, redoButton]
            {
                undoButton->setEnabled(canvas_->canUndo());
                redoButton->setEnabled(canvas_->canRedo());
            });

        auto *undoShortcut = new QShortcut(QKeySequence::Undo, this);
        auto *redoShortcut = new QShortcut(QKeySequence::Redo, this);
        connect(undoShortcut,
            &QShortcut::activated,
            this,
            [this]
            {
                canvas_->undo();
            });
        connect(redoShortcut,
            &QShortcut::activated,
            this,
            [this]
            {
                canvas_->redo();
            });
    }

    ReviewResult ReviewDialog::reviewResult() const
    {
        ReviewResult res;
        res.decision = decision_;
        res.finalBoxes = canvas_->finalBoxes();
        return res;
    }

    void ReviewDialog::reject()
    {
        decision_ = ReviewDecision::DoNotSave;
        QDialog::reject();
    }
}
