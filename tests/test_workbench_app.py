import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtWidgets import QApplication
from matplotlib.figure import Figure

from tools.workbench.app import _ResponsiveFigureCanvas


def test_responsive_figure_canvas_resizes_figure_to_widget_viewport() -> None:
    application = QApplication.instance() or QApplication([])
    figure = Figure(figsize=(15, 9), constrained_layout=True)
    widget = _ResponsiveFigureCanvas(figure)

    try:
        widget.resize(900, 420)
        widget.show()
        application.processEvents()

        figure_width_pixels = figure.get_size_inches()[0] * figure.get_dpi()
        figure_height_pixels = figure.get_size_inches()[1] * figure.get_dpi()

        assert abs(figure_width_pixels - widget.width()) <= 2.0
        assert abs(figure_height_pixels - widget.height()) <= 2.0
    finally:
        widget.close()
        application.processEvents()
