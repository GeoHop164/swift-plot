from pathlib import Path
import tkinter as tk
from tkinter import filedialog

import matplotlib as mpl
import matplotlib.pyplot as plt
import pandas as pd
import questionary


# Matplotlib performance tweaks
mpl.rcParams["path.simplify"] = True
mpl.rcParams["path.simplify_threshold"] = 1.0
mpl.rcParams["agg.path.chunksize"] = 50000


def pick_file():
    root = tk.Tk()
    root.withdraw()

    file_path = filedialog.askopenfilename(
        title="Select CSV or Excel file",
        filetypes=[
            ("CSV Files", "*.csv"),
            ("Excel Files", "*.xlsx *.xls"),
            ("All Files", "*.*"),
        ],
    )

    if not file_path:
        raise SystemExit("No file selected.")

    return file_path


def get_columns(file_path):
    ext = Path(file_path).suffix.lower()

    print("Reading headers...")

    if ext == ".csv":
        return pd.read_csv(file_path, nrows=0).columns.tolist()

    return pd.read_excel(file_path, nrows=0).columns.tolist()


def load_data(file_path, columns):
    ext = Path(file_path).suffix.lower()

    print("Loading data...")

    if ext == ".csv":

        try:
            return pd.read_csv(
                file_path,
                usecols=columns,
                engine="pyarrow",
            )

        except Exception as exc:
            print(f"PyArrow failed: {exc}")
            print("Falling back to pandas C engine")

            return pd.read_csv(
                file_path,
                usecols=columns,
                engine="c",
                low_memory=False,
            )

    return pd.read_excel(
        file_path,
        usecols=columns,
    )


def main():

    file_path = pick_file()

    columns = get_columns(file_path)

    print(f"Found {len(columns)} columns")

    x_choices = ["[Row Number]"] + columns

    x_col = questionary.select(
        "Select X axis",
        choices=x_choices,
    ).ask()

    if not x_col:
        return

    y_cols = questionary.checkbox(
        "Select Y column(s)",
        choices=columns,
        validate=lambda x: len(x) > 0 or "Select at least one column",
    ).ask()

    if not y_cols:
        return

    scale_mode = questionary.select(
        "Y-axis mode",
        choices=[
            "Shared Y Axis",
            "Separate Y Axes",
        ],
    ).ask()

    cols_to_load = list(y_cols)

    if x_col != "[Row Number]":
        cols_to_load.insert(0, x_col)

    cols_to_load = list(dict.fromkeys(cols_to_load))

    df = load_data(
        file_path,
        cols_to_load,
    )

    print(f"Loaded {len(df):,} rows")

    if x_col == "[Row Number]":
        x = df.index
        x_label = "Row Number"
    else:
        x = df[x_col]
        x_label = x_col

    fig, ax = plt.subplots(figsize=(14, 8))

    if scale_mode == "Shared Y Axis":

        for col in y_cols:

            y = pd.to_numeric(
                df[col],
                errors="coerce",
            )

            ax.plot(
                x,
                y,
                linewidth=0.8,
                label=col,
            )

        ax.legend()

    else:

        colours = plt.rcParams["axes.prop_cycle"].by_key()["color"]

        axes = []

        for i, col in enumerate(y_cols):

            if i == 0:
                current_ax = ax
            else:
                current_ax = ax.twinx()

                if i > 1:
                    current_ax.spines["right"].set_position(
                        ("axes", 1 + ((i - 1) * 0.08))
                    )

            colour = colours[i % len(colours)]

            y = pd.to_numeric(
                df[col],
                errors="coerce",
            )

            current_ax.plot(
                x,
                y,
                color=colour,
                linewidth=0.8,
                label=col,
            )

            current_ax.set_ylabel(
                col,
                color=colour,
            )

            current_ax.tick_params(
                axis="y",
                colors=colour,
            )

            axes.append(current_ax)

        handles = []
        labels = []

        for axis in axes:
            h, l = axis.get_legend_handles_labels()
            handles.extend(h)
            labels.extend(l)

        ax.legend(
            handles,
            labels,
            loc="best",
        )

    ax.set_xlabel(x_label)
    ax.grid(True, alpha=0.3)

    plt.tight_layout()
    plt.show()


if __name__ == "__main__":
    main()