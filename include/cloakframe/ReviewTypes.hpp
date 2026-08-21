#pragma once

#include <QMetaType>
#include <QRectF>
#include <QVector>

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
}

Q_DECLARE_METATYPE(cloakframe::ReviewResult)
