#pragma once

#include "cloakframe/Mosaic.hpp"
#include "cloakframe/ReviewTypes.hpp"

#include <QDialog>
#include <QImage>
#include <QRectF>
#include <QVector>

class QKeyEvent;
class QLabel;
class QMouseEvent;
class QPaintEvent;
class QPushButton;

namespace cloakframe
{
    class ReviewCanvas;

    struct ReviewPreviewSpec
    {
        AnonymizationMethod method = AnonymizationMethod::Mosaic;
        cv::Mat customImage;
        MaskShape shape = MaskShape::Rectangle;
        bool softEdges = false;
        int blockSize = 14;
        float padding = 0.18F;
        double previewScale = 1.0;
    };

    class ReviewDialog final : public QDialog
    {
        Q_OBJECT

    public:
        ReviewDialog(const QImage &image,
            const QString &sourceName,
            const QVector<QRectF> &detected,
            int currentIndex,
            int total,
            bool preserveMetadata,
            const ReviewPreviewSpec &previewSpec,
            QWidget *parent = nullptr);

        [[nodiscard]] ReviewResult reviewResult() const;

        void reject() override;

    protected:
        void keyPressEvent(QKeyEvent *event) override;

    private:
        ReviewCanvas *canvas_ = nullptr;
        QLabel *hintLabel_ = nullptr;
        ReviewDecision decision_ = ReviewDecision::Save;
    };
}
