use std::collections::HashMap;
use std::env::{VarError, var};

use serde::{Serialize, Serializer};
use serde::ser::SerializeMap;
use handlebars::{Handlebars, TemplateRenderError};
#[cfg(feature = "rustc_version")]
use rustc_version::{Error as RustcError, version_meta};

use manifest::Substitution;


quick_error! {
    #[derive(Debug)]
    pub enum TemplateGenerationError {
        #[cfg(feature = "rustc_version")]
        RustVersionError(err: RustcError) {
            from()
            description("rustc version error")
            display("Unable to determine rustc version '{}'", err)
        }
        EnvError(err: VarError) {
            from()
            description("environment variable error")
            display("Unable to read environment variable '{}'", err)
        }
    }
}


struct Data<'a> {
    version: Option<&'a str>,
    substitutions: &'a HashMap<String, String>,
}


impl<'a> Serialize for Data<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (version, count) = match self.version {
            Some(v) => {
                if let Some(v) = self.substitutions.get("version") {
                    (Some(v.as_ref()), 0)
                } else {
                    (Some(v), 1)
                }
            }
            None => (None, 0),
        };
        let mut map = serializer.serialize_map(
            Some(self.substitutions.len() + count),
        )?;
        if let Some(version) = version {
            map.serialize_entry("version", version)?;
        }
        for (k, v) in self.substitutions {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}


pub struct TemplateEngine {
    engine: Handlebars,
    substitutions: HashMap<String, String>,
}

impl TemplateEngine {
    pub fn new(
        substitutions: HashMap<String, Substitution>,
    ) -> Result<Self, TemplateGenerationError> {
        let mut resolved_subs = HashMap::new();
        Self::register_rustc_helpers(&mut resolved_subs)?;
        for (name, sub) in substitutions.into_iter() {
            resolved_subs.insert(
                name,
                match sub {
                    Substitution::EnvironmentVariable(key) => var(key)?,
                    Substitution::Value(val) => val,
                },
            );
        }

        Ok(TemplateEngine {
            engine: Handlebars::new(),
            substitutions: resolved_subs,
        })
    }

    #[cfg(feature = "rustc_version")]
    fn register_rustc_helpers(
        substitutions: &mut HashMap<String, String>,
    ) -> Result<(), TemplateGenerationError> {
        let version = version_meta()?;
        substitutions.insert("rustc_short_version".into(), version.short_version_string);
        Ok(())
    }

    #[cfg(not(feature = "rustc_version"))]
    fn register_rustc_helpers(substitutions: &mut HashMap<String, String>) {}

    pub fn render(
        &self,
        template: &str,
        version: Option<&str>,
    ) -> Result<String, TemplateRenderError> {
        self.engine.template_render(
            template,
            &Data {
                version,
                substitutions: &self.substitutions,
            },
        )
    }
}

#[cfg(test)]
mod test {
    use std::env::{set_var, remove_var};
    use std::collections::HashMap;

    use manifest::Substitution;
    use super::TemplateEngine;

    fn test_simple(t: &TemplateEngine) {
        assert_eq!(t.render("", None).unwrap(), "");
        assert_eq!(t.render("", Some("10".into())).unwrap(), "");

        assert_eq!(t.render("foo", None).unwrap(), "foo");
        assert_eq!(t.render("foo", Some("10".into())).unwrap(), "foo");


        assert_eq!(t.render("{{version}}", Some("10".into())).unwrap(), "10");
        assert_eq!(
            t.render("foo{{version}}", Some("10".into())).unwrap(),
            "foo10"
        );
    }

    #[test]
    fn test_version() {
        let t = TemplateEngine::new(HashMap::new()).unwrap();
        test_simple(&t);
    }

    #[test]
    fn test_value() {
        let mut map = HashMap::new();
        map.insert(
            "dhl_val".into(),
            Substitution::Value("dhl_test_value".into()),
        );

        let t = TemplateEngine::new(map).unwrap();
        test_simple(&t);
        assert_eq!(t.render("{{dhl_val}}", None).unwrap(), "dhl_test_value");
        assert_eq!(
            t.render("foo{{dhl_val}}", None).unwrap(),
            "foodhl_test_value"
        );
    }

    #[test]
    fn test_env() {
        set_var("DHL_TEST_ENV_VAR", "dhl_test_env_val");

        let mut map = HashMap::new();
        map.insert(
            "dhl_var".to_owned(),
            Substitution::EnvironmentVariable("DHL_TEST_ENV_VAR".to_owned()),
        );

        let t = TemplateEngine::new(map).unwrap();
        test_simple(&t);
        assert_eq!(t.render("{{dhl_var}}", None).unwrap(), "dhl_test_env_val");
        assert_eq!(
            t.render("foo{{dhl_var}}", None).unwrap(),
            "foodhl_test_env_val"
        );

        remove_var("DHL_TEST_ENV_VAR");
    }
}
