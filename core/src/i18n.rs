//! Localisation (Polish / English).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lang {
    #[default]
    En,
    Pl,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Pl => "Polski",
            Lang::En => "English",
        }
    }

    /// Language from the operating system locale.
    pub fn detect() -> Lang {
        match sys_locale::get_locale() {
            Some(l) if l.to_ascii_lowercase().starts_with("pl") => Lang::Pl,
            _ => Lang::En,
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        match s.to_ascii_lowercase().as_str() {
            "pl" | "polski" | "polish" => Some(Lang::Pl),
            "en" | "english" => Some(Lang::En),
            _ => None,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(l: Lang) {
    LANG.store(if l == Lang::Pl { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::Pl
    } else {
        Lang::En
    }
}

macro_rules! keys {
    ($($k:ident => $pl:expr, $en:expr;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key { $($k),* }

        pub fn tr(k: Key) -> &'static str {
            let pl = lang() == Lang::Pl;
            match k {
                $(Key::$k => if pl { $pl } else { $en }),*
            }
        }
    };
}

keys! {
    MenuFile => "Plik", "File";
    MenuEdit => "Edycja", "Edit";
    Undo => "Cofnij", "Undo";
    Redo => "Ponów", "Redo";
    UndoRedo => "Cofnij / ponów", "Undo / redo";
    Shortcuts => "Skróty klawiszowe", "Keyboard shortcuts";
    IdeaBy => "Pomysł i wykonanie:", "Idea and development:";
    NotExported => "niewyeksportowane", "not exported";
    ExitTitle => "Zamknąć Panoramer?", "Quit Panoramer?";
    ExitQuestion => "Panorama nie została wyeksportowana po ostatnich zmianach. Czy na pewno chcesz wyjść? Niezapisana praca zostanie utracona.",
        "The panorama has not been exported since the last changes. Do you really want to quit? Unsaved work will be lost.";
    ExitRendering => "Trwa renderowanie – wyjście je przerwie.", "Rendering is in progress – quitting will abort it.";
    ExitDiscard => "Wyjdź bez zapisywania", "Quit without saving";
    SecView => "Widok i orientacja", "View & orientation";
    YawHint => "Obrót w poziomie – który kierunek jest środkiem panoramy", "Horizontal rotation – which direction is the centre of the panorama";
    PitchHint => "Pochylenie w górę / w dół", "Tilt up / down";
    RollHint => "Przechylenie (obrót wokół osi patrzenia)", "Roll (rotation around the viewing axis)";
    RotateLeft => "Obróć o 90° w lewo", "Rotate 90° left";
    RotateRight => "Obróć o 90° w prawo", "Rotate 90° right";
    Rotate180 => "Obróć o 180° (do góry nogami)", "Rotate 180° (upside down)";
    FlipH => "Odbicie lustrzane w poziomie", "Mirror horizontally";
    FlipV => "Odbicie lustrzane w pionie", "Mirror vertically";
    FlipShortH => "Odbij ↔", "Mirror ↔";
    FlipShortV => "Odbij ↕", "Mirror ↕";
    ToolRotate => "Obracaj", "Rotate";
    ToolRotateHint => "Przeciągnij, aby obrócić panoramę (chwycony punkt podąża za kursorem). Shift lub prawy przycisk: przechylenie. Kółko: zoom.",
        "Drag to rotate the panorama (the grabbed point follows the cursor). Shift or right button: roll. Wheel: zoom.";
    ToolPan => "Przesuń", "Pan";
    ToolPanHint => "Przesuwaj powiększony podgląd (również środkowym przyciskiem myszy)", "Pan the zoomed preview (also with the middle mouse button)";
    ToolCenter => "Środek", "Centre";
    ToolCenterHint => "Kliknij punkt, który ma być środkiem panoramy", "Click the point that should become the centre of the panorama";
    ToolHorizon => "Poziom", "Horizon";
    ToolHorizonHint => "Kliknij dwa punkty na linii horyzontu – zostanie wypoziomowany", "Click two points on the horizon – it will be levelled";
    ToolVertical => "Pion", "Vertical";
    ToolVerticalHint => "Kliknij dwa punkty na linii, która powinna być pionowa (np. krawędź budynku)", "Click two points on a line that should be vertical (e.g. a building edge)";
    ToolCrop => "Kadruj", "Crop";
    ToolCropHint => "Narysuj prostokąt kadru lub przeciągnij jego krawędzie i narożniki", "Draw the crop rectangle or drag its edges and corners";
    PickFirst => "Kliknij pierwszy punkt na horyzoncie", "Click the first point on the horizon";
    PickFirstV => "Kliknij pierwszy punkt na pionowej linii", "Click the first point on the vertical line";
    PickSecond => "Kliknij drugi punkt (prawy przycisk – anuluj)", "Click the second point (right click – cancel)";
    ZoomIn => "Powiększ", "Zoom in";
    ZoomOut => "Pomniejsz", "Zoom out";
    ZoomFit => "Dopasuj do okna", "Fit to window";
    Grid => "Siatka", "Grid";
    GridHint => "Siatka współrzędnych: pomarańczowa linia to prawdziwy horyzont", "Coordinate grid: the orange line is the true horizon";
    Outlines => "Obrysy zdjęć", "Image outlines";
    OutlinesHint => "Pokaż granice i numery poszczególnych zdjęć", "Show the borders and numbers of the individual images";
    NavControls => "Panel nawigacji na podglądzie", "On-screen navigation controls";
    Navigation => "Nawigacja (przytrzymaj)", "Navigate (hold)";
    NavLeft => "Obracaj w lewo", "Rotate left";
    NavRight => "Obracaj w prawo", "Rotate right";
    NavUp => "Pochyl w górę", "Tilt up";
    NavDown => "Pochyl w dół", "Tilt down";
    NavRollL => "Przechyl w lewo", "Roll left";
    NavRollR => "Przechyl w prawo", "Roll right";
    Lon => "Dł.", "Lon";
    Lat => "Szer.", "Lat";
    PreviewQuality => "Jakość podglądu", "Preview quality";
    QualityFast => "Szybka", "Fast";
    QualityNormal => "Normalna", "Normal";
    QualityHigh => "Wysoka", "High";
    Selected => "Wybrane", "Selected";
    SelectAll => "Zaznacz wszystkie", "Select all";
    SelectNone => "Odznacz", "Select none";
    SortByName => "Sortuj", "Sort";
    UseImage => "Użyj tego zdjęcia w panoramie", "Use this image in the panorama";
    CenterOnImage => "Ustaw jako środek panoramy", "Set as panorama centre";
    Disable => "Wyłącz", "Disable";
    Enable => "Włącz", "Enable";
    ShowInExplorer => "Pokaż w Eksploratorze", "Show in Explorer";
    AlignError => "Błąd dopasowania", "Alignment error";
    Pairs => "Pary zdjęć", "Image pairs";
    StepAdd => "Dodaj zdjęcia", "Add images";
    StepAlign => "Dopasuj", "Align";
    StepRender => "Ustaw widok i zapisz", "Adjust & save";
    ScYawPitch => "Obrót i pochylenie o 1°", "Yaw / pitch by 1°";
    ScRoll => "Przechylenie", "Roll";
    ScShift => "z klawiszem – krok 10° / szybciej", "with a key – 10° steps / faster";
    ScTools => "Narzędzia: obracaj, przesuń, środek, poziom, pion, kadruj", "Tools: rotate, pan, centre, horizon, vertical, crop";
    ScOverlays => "Siatka / obrysy zdjęć", "Grid / image outlines";
    ScWheel => "Kółko myszy", "Mouse wheel";
    ScZoom => "Zoom podglądu", "Preview zoom";
    ScMiddle => "Środkowy przycisk", "Middle button";
    ScRightDrag => "Prawy przycisk + przeciąganie", "Right button + drag";
    MenuLanguage => "Język", "Language";
    MenuView => "Widok", "View";
    MenuHelp => "Pomoc", "Help";
    AddImages => "Dodaj zdjęcia…", "Add images…";
    AddFolder => "Dodaj folder…", "Add folder…";
    ClearAll => "Usuń wszystkie zdjęcia", "Remove all images";
    Exit => "Zakończ", "Exit";
    ShowLog => "Dziennik zdarzeń", "Event log";
    About => "O programie", "About";
    AboutText => "Profesjonalne narzędzie do łączenia zdjęć w panoramy: sferyczne 360°, cylindryczne, prostoliniowe, Panini, little planet i inne. Obsługuje pliki RAW (ARW, NEF, CR2, DNG, RAF, ORF, RW2…) i zapisuje wynik jako liniowy DNG, TIFF, PNG lub JPEG.",
        "Professional panorama stitcher: spherical 360°, cylindrical, rectilinear, Panini, little planet and more. Reads RAW files (ARW, NEF, CR2, DNG, RAF, ORF, RW2…) and writes linear DNG, TIFF, PNG or JPEG.";
    ProjectPage => "Strona projektu (GitHub)", "Project page (GitHub)";
    Log => "Dziennik", "Log";
    ClearLog => "Wyczyść", "Clear";
    Images => "Zdjęcia", "Images";
    DropHint => "Przeciągnij tutaj zdjęcia lub folder", "Drag images or a folder here";
    DropHere => "Upuść, aby dodać zdjęcia", "Drop to add images";
    SupportedFormats => "JPEG, TIFF, PNG, WebP oraz RAW: ARW, NEF, CR2, DNG, RAF, ORF, RW2, PEF, SRW…",
        "JPEG, TIFF, PNG, WebP and RAW: ARW, NEF, CR2, DNG, RAF, ORF, RW2, PEF, SRW…";
    FilterImages => "Obrazy i RAW", "Images and RAW";
    Remove => "Usuń z listy", "Remove from list";
    StatusAligned => "dopasowane", "aligned";
    StatusNotConnected => "brak połączenia z resztą", "not connected";
    SecAlignment => "Obiektyw i dopasowanie", "Lens & alignment";
    SecProjection => "Projekcja", "Projection";
    SecBlending => "Mieszanie i korekcja", "Blending & correction";
    SecCrop => "Kadrowanie", "Crop";
    SecOutput => "Wynik", "Output";
    LensType => "Obiektyw", "Lens";
    LensRectilinear => "Prostoliniowy (standard)", "Rectilinear (standard)";
    LensFisheyeEquidistant => "Rybie oko (równoodległościowe)", "Fisheye (equidistant)";
    LensFisheyeEquisolid => "Rybie oko (równopowierzchniowe)", "Fisheye (equisolid)";
    LensFisheyeStereographic => "Rybie oko (stereograficzne)", "Fisheye (stereographic)";
    FieldOfView => "Kąt widzenia (poziomy)", "Horizontal field of view";
    Manual => "ręcznie", "manual";
    FovHint => "Domyślnie ogniskowa jest odczytywana z EXIF lub szacowana automatycznie. Ustaw ręcznie, jeśli brak danych EXIF (np. obiektyw manualny).",
        "By default the focal length is read from EXIF or estimated automatically. Set it manually when EXIF is missing (e.g. manual lenses).";
    Features => "Punkty charakterystyczne", "Feature points";
    Matching => "Dopasowywanie par", "Pair matching";
    StrategyAuto => "Automatyczne", "Automatic";
    StrategyAll => "Każde z każdym", "All pairs";
    StrategySequential => "Sekwencyjne (sąsiednie)", "Sequential (neighbours)";
    Neighbours => "Sąsiedzi", "Neighbours";
    RansacThreshold => "Próg RANSAC", "RANSAC threshold";
    OptimizeFocal => "Optymalizuj ogniskową (bundle adjustment)", "Optimise focal length (bundle adjustment)";
    Straighten => "Prostuj horyzont automatycznie", "Automatically level the horizon";
    Align => "Dopasuj zdjęcia", "Align images";
    Aligned => "Dopasowano", "Aligned";
    FullSphere => "Pełna sfera 360° × 180°", "Full 360° × 180° sphere";
    FullSphereHint => "Płótno obejmie całą sferę (proporcje 2:1), nawet jeśli zdjęcia jej nie pokrywają. Wymagane przez większość przeglądarek VR.",
        "The canvas covers the whole sphere (2:1), even if the images do not. Required by most VR viewers.";
    Yaw => "Odchylenie (yaw)", "Yaw";
    Pitch => "Pochylenie (pitch)", "Pitch";
    Roll => "Przechylenie (roll)", "Roll";
    ResetView => "Resetuj widok", "Reset view";
    DragHint => "lub przeciągnij podgląd", "or drag the preview";
    CircularFisheye => "Okrągły kadr rybiego oka (maska koła)", "Circular fisheye frame (circle mask)";
    BlendMode => "Mieszanie", "Blending";
    BlendMultiband => "Wielopasmowe (najlepsze)", "Multi-band (best)";
    BlendFeather => "Wtapianie liniowe", "Linear feather";
    BlendSeams => "Ostre szwy", "Hard seams";
    Bands => "Liczba pasm", "Bands";
    AutoShort => "auto", "auto";
    ExposureComp => "Wyrównaj ekspozycję", "Exposure compensation";
    ColorComp => "Wyrównaj balans kolorów", "Colour balance compensation";
    VignettingComp => "Usuń winietowanie", "Vignetting correction";
    Bicubic => "Interpolacja dwusześcienna", "Bicubic interpolation";
    CropAuto => "Automatyczne", "Automatic";
    CropNone => "Brak", "None";
    CropManual => "Ręczne", "Manual";
    Left => "Lewo", "Left";
    Top => "Góra", "Top";
    Right => "Prawo", "Right";
    Bottom => "Dół", "Bottom";
    CropFromAuto => "Zacznij od kadru automatycznego", "Start from automatic crop";
    Format => "Format", "Format";
    FormatAuto => "Automatyczny", "Automatic";
    DngDesc => "liniowy, 16-bit float", "linear, 16-bit float";
    Quality => "Jakość JPEG", "JPEG quality";
    Size => "Rozdzielczość", "Resolution";
    MaxMegapixels => "Limit rozmiaru", "Size limit";
    Transparent => "Przezroczyste tło", "Transparent background";
    Gpano => "Metadane panoramy 360° (GPano)", "360° panorama metadata (GPano)";
    GpanoHint => "Pozwala Google Photos, Facebookowi i przeglądarkom VR rozpoznać zdjęcie jako panoramę sferyczną.",
        "Lets Google Photos, Facebook and VR viewers recognise the image as a spherical panorama.";
    OpenFolder => "Pokaż plik po zapisaniu", "Reveal file after saving";
    Estimated => "Wynik", "Output";
    RenderSave => "Renderuj i zapisz…", "Render & save…";
    Cancel => "Anuluj", "Cancel";
    Cancelled => "Anulowano", "Cancelled";
    Ready => "Gotowy", "Ready";
    PreviewLabel => "Podgląd", "Preview";
    WelcomeAdd => "Dodaj zdjęcia, aby rozpocząć (lub przeciągnij je do okna)", "Add images to begin (or drop them onto the window)";
    WelcomeAlign => "Kliknij „Dopasuj zdjęcia”, aby połączyć je w panoramę", "Click “Align images” to build the panorama";
    StageLoading => "Wczytywanie zdjęć", "Loading images";
    StageFeatures => "Wykrywanie punktów charakterystycznych", "Detecting features";
    StageMatching => "Dopasowywanie i optymalizacja", "Matching and optimisation";
    StagePhotometric => "Kalibracja fotometryczna", "Photometric calibration";
    StagePreview => "Generowanie podglądu…", "Generating preview…";
    StageRendering => "Renderowanie", "Rendering";
    StageSaving => "Zapisywanie pliku", "Saving file";
    LoadedImages => "Wczytano zdjęć:", "Images loaded:";
    AlignDone => "Dopasowanie zakończone", "Alignment finished";
    AlignFailed => "Nie udało się dopasować", "Alignment failed";
    PreviewFailed => "Błąd podglądu", "Preview failed";
    RenderFailed => "Błąd renderowania", "Rendering failed";
    Saved => "Zapisano", "Saved";
    WarnUnaligned => "Część zdjęć nie została połączona z panoramą (za mało wspólnych punktów).", "Some images could not be connected to the panorama (not enough common points).";
    FocalManual => "ogniskowa ręczna", "manual focal length";
    FocalExif => "ogniskowa z EXIF", "focal length from EXIF";
    FocalEstimated => "ogniskowa oszacowana", "estimated focal length";
    FocalDefault => "ogniskowa domyślna", "default focal length";
    ErrNotEnough => "Potrzebne są co najmniej 2 zdjęcia.", "At least 2 images are required.";
    ErrNoOverlap => "Nie znaleziono wspólnych obszarów między zdjęciami. Sprawdź, czy zdjęcia nachodzą na siebie (min. 20–30%) i czy ustawiono właściwy obiektyw.",
        "No overlap found between the images. Make sure they overlap (at least 20–30%) and the lens type is correct.";
    ErrEmptyCanvas => "Wynikowa panorama jest pusta dla tej projekcji.", "The panorama is empty for this projection.";
    ErrTooLarge => "Wynik jest zbyt duży. Zmniejsz rozdzielczość lub limit rozmiaru.", "The result is too large. Reduce the resolution or the size limit.";
    ProjEquirect => "Sferyczna (equirectangular)", "Spherical (equirectangular)";
    ProjCylindrical => "Cylindryczna", "Cylindrical";
    ProjMercator => "Mercator", "Mercator";
    ProjRectilinear => "Prostoliniowa (perspektywa)", "Rectilinear (perspective)";
    ProjPanini => "Panini", "Panini";
    ProjFisheye => "Rybie oko", "Fisheye";
    ProjStereographic => "Stereograficzna", "Stereographic";
    ProjLittlePlanet => "Mała planeta", "Little planet";
    ProjOrthographic => "Ortograficzna", "Orthographic";
    ProjTransverseMercator => "Poprzeczny Mercator (pionowa)", "Transverse Mercator (vertical)";
    ProjSinusoidal => "Sinusoidalna", "Sinusoidal";
    DescEquirect => "Standard dla panoram 360° i VR. Pokazuje do 360° × 180°.", "Standard for 360° and VR panoramas. Shows up to 360° × 180°.";
    DescCylindrical => "Klasyczna panorama pozioma; proste pionowe linie, łagodne krzywizny.", "Classic horizontal panorama; vertical lines stay straight.";
    DescMercator => "Jak cylindryczna, ale z mniejszym rozciąganiem w pionie.", "Like cylindrical but with less vertical stretching.";
    DescRectilinear => "Wszystkie linie proste pozostają proste; do ok. 120° pola widzenia.", "All straight lines stay straight; up to about 120° field of view.";
    DescPanini => "Szerokie widoki architektury i wnętrz z prostymi pionami.", "Wide architectural and interior views with straight verticals.";
    DescFisheye => "Efekt obiektywu rybie oko; obejmuje bardzo szerokie pola widzenia.", "Fisheye look; handles very wide fields of view.";
    DescStereographic => "Zachowuje kształty lokalnie; dobra dla bardzo szerokich kadrów.", "Conformal; good for very wide views.";
    DescLittlePlanet => "Efekt „małej planety” – najlepiej z pełnej panoramy 360°.", "“Little planet” effect – best with a full 360° panorama.";
    DescOrthographic => "Widok półkuli jak z daleka (kula lustrzana).", "Hemisphere seen from afar (mirror ball look).";
    DescTransverseMercator => "Dla pionowych panoram (np. wysokie budynki, wodospady).", "For vertical panoramas (tall buildings, waterfalls).";
    DescSinusoidal => "Równopowierzchniowa projekcja całej sfery.", "Equal-area projection of the whole sphere.";
}
