use fluent_bundle::FluentResource;
use fluent_bundle::bundle::FluentBundle;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use unic_langid::{LanguageIdentifier, langid};

use crate::config::{self, REQUIRED_TRANSLATION_RESOURCES};

type TranslationBundle = FluentBundle<FluentResource, intl_memoizer::concurrent::IntlLangMemoizer>;

#[derive(Debug)]
pub struct TranslationLoadError {
    relative_path: PathBuf,
    resolved_path: Option<PathBuf>,
    stage: &'static str,
    detail: String,
}

impl TranslationLoadError {
    fn new(
        relative_path: &Path,
        resolved_path: Option<&Path>,
        stage: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            relative_path: relative_path.to_path_buf(),
            resolved_path: resolved_path.map(Path::to_path_buf),
            stage,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for TranslationLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Resource:")?;
        writeln!(formatter, "{}", self.relative_path.display())?;
        if let Some(path) = &self.resolved_path {
            writeln!(formatter)?;
            writeln!(formatter, "Attempted location:")?;
            writeln!(formatter, "{}", path.display())?;
        }
        writeln!(formatter)?;
        write!(formatter, "Error:\n{}: {}", self.stage, self.detail)
    }
}

impl std::error::Error for TranslationLoadError {}

struct TranslationBundles {
    bundles: [TranslationBundle; 6],
}

impl TranslationBundles {
    fn load() -> Result<Self, TranslationLoadError> {
        let resources = config::runtime_resources().map_err(|error| {
            TranslationLoadError::new(error.relative_path(), None, "resolution", error.to_string())
        })?;
        let mut bundles = Vec::with_capacity(REQUIRED_TRANSLATION_RESOURCES.len());
        for (relative, language) in REQUIRED_TRANSLATION_RESOURCES.iter().zip(Language::ALL) {
            let relative = Path::new(relative);
            let resolved = resources.path(relative).ok_or_else(|| {
                TranslationLoadError::new(
                    relative,
                    None,
                    "resolution",
                    "validated runtime resource is missing from the catalog",
                )
            })?;
            bundles.push(load_bundle_from_path(
                relative,
                resolved,
                language.language_identifier(),
            )?);
        }
        let bundles = match bundles.try_into() {
            Ok(bundles) => bundles,
            Err(_) => unreachable!("the required translation list contains six entries"),
        };
        Ok(Self { bundles })
    }

    fn bundle(&self, language: Language) -> &TranslationBundle {
        &self.bundles[language.index()]
    }
}

static TRANSLATION_BUNDLES: LazyLock<Result<TranslationBundles, TranslationLoadError>> =
    LazyLock::new(TranslationBundles::load);

fn load_bundle_from_path(
    relative_path: &Path,
    resolved_path: &Path,
    language_identifier: LanguageIdentifier,
) -> Result<TranslationBundle, TranslationLoadError> {
    let file = std::fs::File::open(resolved_path).map_err(|error| {
        TranslationLoadError::new(
            relative_path,
            Some(resolved_path),
            "open",
            error.to_string(),
        )
    })?;
    let mut reader = std::io::BufReader::new(file);
    let mut source = String::new();
    reader.read_to_string(&mut source).map_err(|error| {
        TranslationLoadError::new(
            relative_path,
            Some(resolved_path),
            "read",
            error.to_string(),
        )
    })?;
    let resource = FluentResource::try_new(source).map_err(|(_, errors)| {
        TranslationLoadError::new(
            relative_path,
            Some(resolved_path),
            "parse",
            format!("{errors:?}"),
        )
    })?;
    let mut bundle = FluentBundle::new_concurrent(vec![language_identifier]);
    bundle.add_resource(resource).map_err(|errors| {
        TranslationLoadError::new(
            relative_path,
            Some(resolved_path),
            "bundle",
            format!("{errors:?}"),
        )
    })?;
    Ok(bundle)
}

pub fn validate_required_translations() -> Result<(), &'static TranslationLoadError> {
    TRANSLATION_BUNDLES.as_ref().map(|_| ())
}

pub fn tr(lang: &Language, key: &str) -> String {
    let bundles = TRANSLATION_BUNDLES
        .as_ref()
        .unwrap_or_else(|error| panic!("Required translations were not validated: {error}"));
    let bundle = bundles.bundle(*lang);
    let msg = bundle
        .get_message(key)
        .unwrap_or_else(|| panic!("{}", ("Missing translation key ".to_owned() + key)));
    let mut errors = vec![];
    let pattern = msg.value().expect("Missing Value.");
    bundle
        .format_pattern(pattern, None, &mut errors)
        .to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Portuguese,
    Spanish,
    French,
    Chinese,
    Dutch,
}

impl Language {
    pub const ALL: [Language; 6] = [
        Language::English,
        Language::Portuguese,
        Language::Spanish,
        Language::French,
        Language::Chinese,
        Language::Dutch,
    ];

    fn index(self) -> usize {
        match self {
            Self::English => 0,
            Self::Portuguese => 1,
            Self::Spanish => 2,
            Self::French => 3,
            Self::Chinese => 4,
            Self::Dutch => 5,
        }
    }

    fn language_identifier(self) -> LanguageIdentifier {
        match self {
            Self::English => langid!("en-US"),
            Self::Portuguese => langid!("pt-BR"),
            Self::Spanish => langid!("es"),
            Self::French => langid!("fr"),
            Self::Chinese => langid!("cn"),
            Self::Dutch => langid!("nl"),
        }
    }
}

impl DisplayTranslated for Language {
    fn to_str_tr(&self) -> &str {
        match self {
            Language::English => "english",
            Language::Portuguese => "portuguese",
            Language::Spanish => "spanish",
            Language::French => "french",
            Language::Chinese => "chinese",
            Language::Dutch => "dutch",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PickListWrapper<D: DisplayTranslated> {
    pub lang: Language,
    pub item: D,
}

pub trait DisplayTranslated {
    fn to_str_tr(&self) -> &str;
}

impl<D: DisplayTranslated> std::fmt::Display for PickListWrapper<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr(&self.lang, self.item.to_str_tr()))
    }
}

impl<D: DisplayTranslated + std::cmp::PartialEq> PartialEq for PickListWrapper<D> {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item
    }
}

impl<D: DisplayTranslated + std::cmp::PartialEq> Eq for PickListWrapper<D> {}

impl PickListWrapper<Language> {
    pub fn get_langs(lang: Language) -> Vec<PickListWrapper<Language>> {
        let mut themes_wrapper = Vec::new();
        for item in Language::ALL {
            themes_wrapper.push(PickListWrapper::<Language> { lang, item });
        }
        themes_wrapper
    }

    pub fn new_lang(lang: Language, item: Language) -> Self {
        Self { lang, item }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{REQUIRED_TRANSLATION_RESOURCES, RuntimeResourceResolver};
    use std::path::{Path, PathBuf};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/cms025b-lang-tests")
                .join(format!(
                    "{label}-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("clock should be after epoch")
                        .as_nanos()
                ));
            std::fs::create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn every_required_translation_resolves_and_parses_through_runtime_policy() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let resolver = RuntimeResourceResolver::new(
            manifest.join("target/debug/chess-material-studio.exe"),
            manifest,
        );

        for (relative, language) in REQUIRED_TRANSLATION_RESOURCES.iter().zip(Language::ALL) {
            let path = resolver
                .resolve(Path::new(relative))
                .expect("translation should resolve through Cargo fallback");
            load_bundle_from_path(Path::new(relative), &path, language.language_identifier())
                .unwrap_or_else(|error| panic!("{relative}: {error}"));
        }
    }

    #[test]
    fn invalid_ftl_returns_explicit_parse_diagnostic() {
        let directory = TestDirectory::new("invalid-ftl");
        let path = directory.0.join("ocp.ftl");
        std::fs::write(&path, "broken = { ???").expect("invalid FTL should be written");
        let relative = Path::new("translations/es/ocp.ftl");

        let error = match load_bundle_from_path(relative, &path, langid!("es")) {
            Ok(_) => panic!("invalid FTL must fail"),
            Err(error) => error,
        };
        let diagnostic = error.to_string();

        assert!(diagnostic.contains("translations/es/ocp.ftl"));
        assert!(diagnostic.contains(&path.display().to_string()));
        assert!(diagnostic.contains("parse"));
    }
}
