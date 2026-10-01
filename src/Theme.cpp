#include "cloakframe/Theme.hpp"

#include <QApplication>
#include <QColor>
#include <QGuiApplication>
#include <QPalette>
#include <QScreen>
#include <QStyleHints>

#include <algorithm>

#ifdef CLOAKFRAME_HAVE_SVG
#include <QPainter>
#include <QPixmap>
#include <QSize>
#include <QSvgRenderer>
#endif

namespace cloakframe
{
    namespace
    {
        struct ThemeColors
        {
            QString windowBg;
            QString text;
            QString subtext;
            QString fieldLabel;
            QString cardBg;
            QString border;
            QString inputBg;
            QString focusBorder;
            QString disabledBg;
            QString disabledText;
            QString arrow;
            QString buttonBg;
            QString buttonHoverBg;
            QString buttonHoverBorder;
            QString buttonPressedBg;
            QString buttonDisabledText;
            QString buttonDisabledBg;
            QString primaryBg;
            QString primaryText;
            QString primaryHoverBg;
            QString primaryPressedBg;
            QString primaryDisabledBg;
            QString primaryDisabledText;
            QString dangerText;
            QString dangerBorder;
            QString dangerHoverBg;
            QString dangerDisabledText;
            QString dangerDisabledBorder;
            QString listItemSelectedBg;
            QString listItemSelectedText;
            QString monoText;
            QString selectionBg;
            QString selectionText;
            QString progressTrack;
            QString progressChunk;
            QString scrollHandle;
            QString scrollHandleHover;
            QString bottomBarBg;
            QString bottomBarBorder;
            QString statusText;
            QString warningText;
            QString toggleText;
            QString toggleHoverText;
            QString invalidBorder;
        };

        ThemeColors lightColors()
        {
            ThemeColors c;
            c.windowBg = "#F7F8FA";
            c.text = "#111827";
            c.subtext = "#6B7280";
            c.fieldLabel = "#4B5563";
            c.cardBg = "#FFFFFF";
            c.border = "#E5E7EB";
            c.inputBg = "#FFFFFF";
            c.focusBorder = "#111827";
            c.disabledBg = "#F3F4F6";
            c.disabledText = "#9CA3AF";
            c.arrow = "#6B7280";
            c.buttonBg = "#FFFFFF";
            c.buttonHoverBg = "#F3F4F6";
            c.buttonHoverBorder = "#D1D5DB";
            c.buttonPressedBg = "#E5E7EB";
            c.buttonDisabledText = "#9CA3AF";
            c.buttonDisabledBg = "#F9FAFB";
            c.primaryBg = "#111827";
            c.primaryText = "#FFFFFF";
            c.primaryHoverBg = "#1F2937";
            c.primaryPressedBg = "#000000";
            c.primaryDisabledBg = "#9CA3AF";
            c.primaryDisabledText = "#FFFFFF";
            c.dangerText = "#DC2626";
            c.dangerBorder = "#FECACA";
            c.dangerHoverBg = "#FEF2F2";
            c.dangerDisabledText = "#9CA3AF";
            c.dangerDisabledBorder = "#E5E7EB";
            c.listItemSelectedBg = "#F3F4F6";
            c.listItemSelectedText = "#111827";
            c.monoText = "#374151";
            c.selectionBg = "#111827";
            c.selectionText = "#FFFFFF";
            c.progressTrack = "#F3F4F6";
            c.progressChunk = "#111827";
            c.scrollHandle = "#D1D5DB";
            c.scrollHandleHover = "#9CA3AF";
            c.bottomBarBg = "#FFFFFF";
            c.bottomBarBorder = "#E5E7EB";
            c.statusText = "#6B7280";
            c.warningText = "#DC2626";
            c.toggleText = "#111827";
            c.toggleHoverText = "#374151";
            c.invalidBorder = "#DC2626";
            return c;
        }

        ThemeColors darkColors()
        {
            ThemeColors c;
            c.windowBg = "#0D1117";
            c.text = "#E6EDF3";
            c.subtext = "#8B949E";
            c.fieldLabel = "#ADBAC7";
            c.cardBg = "#161B22";
            c.border = "#30363D";
            c.inputBg = "#0D1117";
            c.focusBorder = "#E6EDF3";
            c.disabledBg = "#21262D";
            c.disabledText = "#6E7681";
            c.arrow = "#8B949E";
            c.buttonBg = "#21262D";
            c.buttonHoverBg = "#30363D";
            c.buttonHoverBorder = "#484F58";
            c.buttonPressedBg = "#161B22";
            c.buttonDisabledText = "#6E7681";
            c.buttonDisabledBg = "#161B22";
            c.primaryBg = "#E6EDF3";
            c.primaryText = "#0D1117";
            c.primaryHoverBg = "#C9D1D9";
            c.primaryPressedBg = "#FFFFFF";
            c.primaryDisabledBg = "#30363D";
            c.primaryDisabledText = "#6E7681";
            c.dangerText = "#F85149";
            c.dangerBorder = "#6E2B2E";
            c.dangerHoverBg = "#2D1A1C";
            c.dangerDisabledText = "#6E7681";
            c.dangerDisabledBorder = "#30363D";
            c.listItemSelectedBg = "#30363D";
            c.listItemSelectedText = "#E6EDF3";
            c.monoText = "#ADBAC7";
            c.selectionBg = "#1F6FEB";
            c.selectionText = "#FFFFFF";
            c.progressTrack = "#21262D";
            c.progressChunk = "#E6EDF3";
            c.scrollHandle = "#30363D";
            c.scrollHandleHover = "#484F58";
            c.bottomBarBg = "#161B22";
            c.bottomBarBorder = "#30363D";
            c.statusText = "#8B949E";
            c.warningText = "#F85149";
            c.toggleText = "#E6EDF3";
            c.toggleHoverText = "#ADBAC7";
            c.invalidBorder = "#F85149";
            return c;
        }

        QString buildStyleSheet(const ThemeColors &c)
        {
            QString s = QStringLiteral(R"(
            QWidget {
                color: @text@;
            }
            QMainWindow, #rootScroll, #rootScroll > QWidget > QWidget {
                background-color: @windowBg@;
            }
            QLabel#titleLabel {
                font-size: 22px;
                font-weight: 600;
                color: @text@;
            }
            QLabel#subtitleLabel {
                font-size: 12px;
                color: @subtext@;
            }
            QLabel[role="sectionTitle"] {
                font-size: 13px;
                font-weight: 600;
                color: @text@;
                letter-spacing: 0.2px;
            }
            QLabel[role="sectionHint"] {
                font-size: 12px;
                color: @subtext@;
            }
            QLabel[role="fieldLabel"] {
                font-size: 12px;
                color: @fieldLabel@;
            }
            QFrame#card {
                background-color: @cardBg@;
                border: 1px solid @border@;
                border-radius: 12px;
            }
            QLineEdit, QComboBox, QSpinBox, QDoubleSpinBox {
                background-color: @inputBg@;
                border: 1px solid @border@;
                border-radius: 8px;
                padding: 6px 10px;
                min-height: 20px;
                selection-background-color: @selectionBg@;
                selection-color: @selectionText@;
            }
            QLineEdit:focus, QComboBox:focus, QSpinBox:focus, QDoubleSpinBox:focus {
                border: 1px solid @focusBorder@;
            }
            QLineEdit:disabled, QComboBox:disabled, QSpinBox:disabled, QDoubleSpinBox:disabled {
                background-color: @disabledBg@;
                color: @disabledText@;
            }
            QComboBox::drop-down {
                border: none;
                width: 22px;
            }
            QComboBox::down-arrow {
                image: none;
                border-left: 4px solid transparent;
                border-right: 4px solid transparent;
                border-top: 5px solid @arrow@;
                margin-right: 10px;
            }
            QPushButton {
                background-color: @buttonBg@;
                border: 1px solid @border@;
                border-radius: 8px;
                padding: 7px 14px;
                color: @text@;
                font-weight: 500;
            }
            QPushButton:hover {
                background-color: @buttonHoverBg@;
                border-color: @buttonHoverBorder@;
            }
            QPushButton:pressed {
                background-color: @buttonPressedBg@;
            }
            QPushButton:disabled {
                color: @buttonDisabledText@;
                background-color: @buttonDisabledBg@;
                border-color: @border@;
            }
            QPushButton#primaryButton {
                background-color: @primaryBg@;
                color: @primaryText@;
                border: 1px solid @primaryBg@;
                padding: 9px 20px;
                font-weight: 600;
            }
            QPushButton#primaryButton:hover {
                background-color: @primaryHoverBg@;
            }
            QPushButton#primaryButton:pressed {
                background-color: @primaryPressedBg@;
            }
            QPushButton#primaryButton:disabled {
                background-color: @primaryDisabledBg@;
                border-color: @primaryDisabledBg@;
                color: @primaryDisabledText@;
            }
            QPushButton#dangerButton {
                background-color: @buttonBg@;
                color: @dangerText@;
                border: 1px solid @dangerBorder@;
            }
            QPushButton#dangerButton:hover {
                background-color: @dangerHoverBg@;
            }
            QPushButton#dangerButton:disabled {
                color: @dangerDisabledText@;
                border-color: @dangerDisabledBorder@;
            }
            QListWidget, QPlainTextEdit {
                background-color: @inputBg@;
                border: 1px solid @border@;
                border-radius: 8px;
                padding: 4px;
            }
            QListWidget::item {
                padding: 6px 8px;
                border-radius: 4px;
            }
            QListWidget::item:selected {
                background-color: @listItemSelectedBg@;
                color: @listItemSelectedText@;
            }
            QPlainTextEdit {
                font-family: "SF Mono", "Menlo", "Consolas", monospace;
                font-size: 12px;
                color: @monoText@;
            }
            QCheckBox {
                spacing: 8px;
                color: @text@;
            }
            QProgressBar {
                background-color: @progressTrack@;
                border: none;
                border-radius: 4px;
                height: 8px;
                text-align: center;
                color: transparent;
            }
            QProgressBar::chunk {
                background-color: @progressChunk@;
                border-radius: 4px;
            }
            QScrollBar:vertical {
                background: transparent;
                width: 10px;
                margin: 2px;
            }
            QScrollBar::handle:vertical {
                background: @scrollHandle@;
                border-radius: 4px;
                min-height: 24px;
            }
            QScrollBar::handle:vertical:hover {
                background: @scrollHandleHover@;
            }
            QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
                height: 0;
            }
            QScrollArea {
                border: none;
                background: transparent;
            }
            QWidget#bottomBar {
                background-color: @bottomBarBg@;
                border-top: 1px solid @bottomBarBorder@;
            }
            QLabel#statusLabel {
                color: @statusText@;
                font-size: 12px;
            }
            QLabel#statusLabel[state="warning"] {
                color: @warningText@;
                font-weight: 600;
            }
            QLineEdit[invalid="true"], QComboBox[invalid="true"],
            QListWidget[invalid="true"] {
                border: 1px solid @invalidBorder@;
            }
            QListWidget[dragActive="true"] {
                border: 1px solid @focusBorder@;
                background-color: @buttonHoverBg@;
            }
            QToolButton#advancedToggle {
                background: transparent;
                border: none;
                padding: 4px 2px;
                font-size: 13px;
                font-weight: 600;
                color: @toggleText@;
            }
            QToolButton#advancedToggle:hover {
                color: @toggleHoverText@;
            }
            QToolButton#settingsButton {
                background: transparent;
                border: none;
                border-radius: 8px;
                font-size: 22px;
                color: @subtext@;
            }
            QToolButton#settingsButton:hover {
                background-color: @buttonHoverBg@;
                color: @text@;
            }
        )");

            const std::pair<const char *, QString> tokens[] = {
                {"@windowBg@", c.windowBg},
                {"@text@", c.text},
                {"@subtext@", c.subtext},
                {"@fieldLabel@", c.fieldLabel},
                {"@cardBg@", c.cardBg},
                {"@border@", c.border},
                {"@inputBg@", c.inputBg},
                {"@focusBorder@", c.focusBorder},
                {"@disabledBg@", c.disabledBg},
                {"@disabledText@", c.disabledText},
                {"@arrow@", c.arrow},
                {"@buttonBg@", c.buttonBg},
                {"@buttonHoverBg@", c.buttonHoverBg},
                {"@buttonHoverBorder@", c.buttonHoverBorder},
                {"@buttonPressedBg@", c.buttonPressedBg},
                {"@buttonDisabledText@", c.buttonDisabledText},
                {"@buttonDisabledBg@", c.buttonDisabledBg},
                {"@primaryBg@", c.primaryBg},
                {"@primaryText@", c.primaryText},
                {"@primaryHoverBg@", c.primaryHoverBg},
                {"@primaryPressedBg@", c.primaryPressedBg},
                {"@primaryDisabledBg@", c.primaryDisabledBg},
                {"@primaryDisabledText@", c.primaryDisabledText},
                {"@dangerText@", c.dangerText},
                {"@dangerBorder@", c.dangerBorder},
                {"@dangerHoverBg@", c.dangerHoverBg},
                {"@dangerDisabledText@", c.dangerDisabledText},
                {"@dangerDisabledBorder@", c.dangerDisabledBorder},
                {"@listItemSelectedBg@", c.listItemSelectedBg},
                {"@listItemSelectedText@", c.listItemSelectedText},
                {"@monoText@", c.monoText},
                {"@selectionBg@", c.selectionBg},
                {"@selectionText@", c.selectionText},
                {"@progressTrack@", c.progressTrack},
                {"@progressChunk@", c.progressChunk},
                {"@scrollHandle@", c.scrollHandle},
                {"@scrollHandleHover@", c.scrollHandleHover},
                {"@bottomBarBg@", c.bottomBarBg},
                {"@bottomBarBorder@", c.bottomBarBorder},
                {"@statusText@", c.statusText},
                {"@warningText@", c.warningText},
                {"@toggleText@", c.toggleText},
                {"@toggleHoverText@", c.toggleHoverText},
                {"@invalidBorder@", c.invalidBorder},
            };
            for (const auto &[key, value] : tokens)
            {
                s.replace(QLatin1String(key), value);
            }
            return s;
        }

        QPalette lightPalette()
        {
            QPalette palette;
            palette.setColor(QPalette::Window, QColor("#F7F8FA"));
            palette.setColor(QPalette::WindowText, QColor("#111827"));
            palette.setColor(QPalette::Base, QColor("#FFFFFF"));
            palette.setColor(QPalette::AlternateBase, QColor("#F3F4F6"));
            palette.setColor(QPalette::Text, QColor("#111827"));
            palette.setColor(QPalette::Button, QColor("#FFFFFF"));
            palette.setColor(QPalette::ButtonText, QColor("#111827"));
            palette.setColor(QPalette::Highlight, QColor("#111827"));
            palette.setColor(QPalette::HighlightedText, QColor("#FFFFFF"));
            palette.setColor(QPalette::PlaceholderText, QColor("#6B7280"));
            palette.setColor(QPalette::ToolTipBase, QColor("#FFFFFF"));
            palette.setColor(QPalette::ToolTipText, QColor("#111827"));
            palette.setColor(QPalette::Disabled, QPalette::Text, QColor("#9CA3AF"));
            palette.setColor(QPalette::Disabled, QPalette::ButtonText, QColor("#9CA3AF"));
            palette.setColor(QPalette::Disabled, QPalette::WindowText, QColor("#9CA3AF"));
            return palette;
        }

        QPalette darkPalette()
        {
            QPalette palette;
            palette.setColor(QPalette::Window, QColor("#0D1117"));
            palette.setColor(QPalette::WindowText, QColor("#E6EDF3"));
            palette.setColor(QPalette::Base, QColor("#0D1117"));
            palette.setColor(QPalette::AlternateBase, QColor("#161B22"));
            palette.setColor(QPalette::Text, QColor("#E6EDF3"));
            palette.setColor(QPalette::Button, QColor("#21262D"));
            palette.setColor(QPalette::ButtonText, QColor("#E6EDF3"));
            palette.setColor(QPalette::Highlight, QColor("#1F6FEB"));
            palette.setColor(QPalette::HighlightedText, QColor("#FFFFFF"));
            palette.setColor(QPalette::PlaceholderText, QColor("#8B949E"));
            palette.setColor(QPalette::ToolTipBase, QColor("#161B22"));
            palette.setColor(QPalette::ToolTipText, QColor("#E6EDF3"));
            palette.setColor(QPalette::Disabled, QPalette::Text, QColor("#6E7681"));
            palette.setColor(QPalette::Disabled, QPalette::ButtonText, QColor("#6E7681"));
            palette.setColor(QPalette::Disabled, QPalette::WindowText, QColor("#6E7681"));
            return palette;
        }
    }

    ThemeMode themeModeFromString(const QString &value)
    {
        if (value.compare(QLatin1String("light"), Qt::CaseInsensitive) == 0)
        {
            return ThemeMode::Light;
        }
        if (value.compare(QLatin1String("dark"), Qt::CaseInsensitive) == 0)
        {
            return ThemeMode::Dark;
        }
        return ThemeMode::System;
    }

    QString themeModeToString(ThemeMode mode)
    {
        switch (mode)
        {
        case ThemeMode::Light:
            return QStringLiteral("light");
        case ThemeMode::Dark:
            return QStringLiteral("dark");
        case ThemeMode::System:
        default:
            return QStringLiteral("system");
        }
    }

    bool systemPrefersDark()
    {
#if QT_VERSION >= QT_VERSION_CHECK(6, 5, 0)
        return QGuiApplication::styleHints()->colorScheme() == Qt::ColorScheme::Dark;
#else
        return false;
#endif
    }

    void applyTheme(QApplication &app, ThemeMode mode)
    {
        const bool dark =
            (mode == ThemeMode::Dark) || (mode == ThemeMode::System && systemPrefersDark());
        const ThemeColors colors = dark ? darkColors() : lightColors();
        app.setPalette(dark ? darkPalette() : lightPalette());
        app.setStyleSheet(buildStyleSheet(colors));
    }

    QIcon settingsGearIcon(ThemeMode mode)
    {
#ifdef CLOAKFRAME_HAVE_SVG
        const bool dark =
            (mode == ThemeMode::Dark) || (mode == ThemeMode::System && systemPrefersDark());
        const QString color = dark ? QStringLiteral("#8B949E") : QStringLiteral("#6B7280");
        const QString svg = QStringLiteral(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' "
            "stroke='%1' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'>"
            "<circle cx='12' cy='12' r='3'/>"
            "<path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06"
            "a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09"
            "A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06"
            "a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09"
            "A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06"
            "a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09"
            "a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06"
            "a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09"
            "a1.65 1.65 0 0 0-1.51 1z'/></svg>")
                                .arg(color);

        constexpr int logical = 20;
        qreal dpr = 1.0;
        for (const QScreen *screen : QGuiApplication::screens())
        {
            dpr = std::max(dpr, screen->devicePixelRatio());
        }
        QPixmap pixmap(QSize(logical, logical) * dpr);
        pixmap.fill(Qt::transparent);
        QSvgRenderer renderer(svg.toUtf8());
        QPainter painter(&pixmap);
        renderer.render(&painter);
        painter.end();
        pixmap.setDevicePixelRatio(dpr);
        return QIcon(pixmap);
#else
        Q_UNUSED(mode);
        return {};
#endif
    }
}
