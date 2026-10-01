#pragma once

#include <QMetaType>
#include <QPointF>
#include <QRectF>
#include <QVector>
#include <qnamespace.h>

#include <algorithm>

namespace cloakframe
{
    enum class ReviewDecision
    {
        Save,
        DoNotSave,
        CopyOriginal,
        CancelAll,
    };

    struct ReviewResult
    {
        ReviewDecision decision = ReviewDecision::Save;
        QVector<QRectF> finalBoxes;
    };

    // True when review took away every region the detectors found and put nothing in their
    // place. The run began with something to cover and is about to write a file that covers
    // none of it, which is worth stopping for.
    //
    // A run that found nothing to begin with is deliberately not this case. An empty detection
    // is ordinary, a photo with no face in it, and stopping for it would teach the reader to
    // click through the one prompt that matters.
    [[nodiscard]] constexpr bool reviewClearedEveryDetection(
        qsizetype detectedCount, qsizetype finalCount) noexcept
    {
        return detectedCount > 0 && finalCount == 0;
    }

    [[nodiscard]] constexpr bool isArrowKey(int key) noexcept
    {
        return key == Qt::Key_Left || key == Qt::Key_Right || key == Qt::Key_Up
               || key == Qt::Key_Down;
    }

    // A box for keyboard placement: square, centered on what is on screen.
    [[nodiscard]] inline QRectF defaultReviewRect(const QRectF &visible, const QRectF &bounds)
    {
        const qreal side = std::max<qreal>(1.0, std::min(bounds.width(), bounds.height()) * 0.15);
        QRectF rect(0, 0, side, side);
        rect.moveCenter(visible.center());
        return rect.intersected(bounds);
    }

    // One arrow-key step of 1% of the longer side: moves `rect`, or with `resize` moves its
    // bottom-right corner. Keys other than the arrows leave it unchanged.
    [[nodiscard]] inline QRectF nudgeReviewRect(
        const QRectF &rect, int key, bool resize, const QRectF &bounds)
    {
        QPointF step;
        switch (key)
        {
        case Qt::Key_Left:
            step = {-1, 0};
            break;
        case Qt::Key_Right:
            step = {1, 0};
            break;
        case Qt::Key_Up:
            step = {0, -1};
            break;
        case Qt::Key_Down:
            step = {0, 1};
            break;
        default:
            return rect;
        }
        const qreal unit = std::max<qreal>(1.0, std::max(bounds.width(), bounds.height()) / 100.0);
        step *= unit;
        if (resize)
        {
            const qreal maxWidth = bounds.right() - rect.left();
            const qreal maxHeight = bounds.bottom() - rect.top();
            QRectF resized = rect;
            resized.setWidth(
                std::max(std::min(rect.width() + step.x(), maxWidth), std::min(unit, maxWidth)));
            resized.setHeight(
                std::max(std::min(rect.height() + step.y(), maxHeight), std::min(unit, maxHeight)));
            return resized;
        }
        QRectF moved = rect.translated(step);
        moved.moveLeft(std::clamp(moved.left(), bounds.left(), bounds.right() - moved.width()));
        moved.moveTop(std::clamp(moved.top(), bounds.top(), bounds.bottom() - moved.height()));
        return moved;
    }
}

Q_DECLARE_METATYPE(cloakframe::ReviewResult)
