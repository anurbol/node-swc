#![feature(box_syntax)]
#![feature(box_patterns)]
#![feature(never_type)]
#![recursion_limit = "2048"]

extern crate failure;
extern crate fxhash;
extern crate hashbrown;
extern crate lazy_static;
extern crate neon;
extern crate neon_serde;
extern crate path_clean;
extern crate swc;

use neon::prelude::*;
use path_clean::clean;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use swc::{
    common::{self, errors::Handler, FileName, FilePathMapping, SourceFile, SourceMap, Spanned},
    config::{Options, ParseOptions},
    ecmascript::ast::Module,
    error::Error,
    Compiler, TransformOutput,
};

fn init(_cx: MethodContext<JsUndefined>) -> NeonResult<ArcCompiler> {
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));

    let handler = Handler::with_tty_emitter(
        common::errors::ColorConfig::Always,
        true,
        false,
        Some(cm.clone()),
    );

    let c = Compiler::new(cm.clone(), handler);

    Ok(Arc::new(c))
}

struct TransformTask {
    c: Arc<Compiler>,
    fm: Arc<SourceFile>,
    options: Options,
}

struct TransformFileTask {
    c: Arc<Compiler>,
    path: PathBuf,
    options: Options,
}

fn complete_output(
    mut cx: TaskContext,
    result: Result<TransformOutput, Error>,
) -> JsResult<JsValue> {
    match result {
        Ok(output) => Ok(neon_serde::to_value(&mut cx, &output)?),
        Err(err) => cx.throw_error(err.to_string()),
    }
}

impl Task for TransformTask {
    type Output = TransformOutput;
    type Error = Error;
    type JsEvent = JsValue;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        self.c
            .process_js_file(self.fm.clone(), self.options.clone())
    }

    fn complete(
        self,
        cx: TaskContext,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        complete_output(cx, result)
    }
}

impl Task for TransformFileTask {
    type Output = TransformOutput;
    type Error = Error;
    type JsEvent = JsValue;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        let fm = self
            .c
            .cm
            .load_file(&self.path)
            .map_err(|err| Error::FailedToReadModule { err })?;

        self.c.process_js_file(fm, self.options.clone())
    }

    fn complete(
        self,
        cx: TaskContext,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        complete_output(cx, result)
    }
}

fn transform(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let source = cx.argument::<JsString>(0)?;
    let options_arg = cx.argument::<JsValue>(1)?;
    let options: Options = neon_serde::from_value(&mut cx, options_arg)?;
    let callback = cx.argument::<JsFunction>(2)?;

    let this = cx.this();
    {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        let fm = c.cm.new_source_file(
            if options.filename.is_empty() {
                FileName::Anon
            } else {
                FileName::Real(options.filename.clone().into())
            },
            source.value(),
        );

        TransformTask {
            c: c.clone(),
            fm,
            options,
        }
        .schedule(callback);
    };

    Ok(cx.undefined().upcast())
}

fn transform_sync(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let source = cx.argument::<JsString>(0)?;
    let options: Options = match cx.argument_opt(1) {
        Some(v) => neon_serde::from_value(&mut cx, v)?,
        None => {
            let obj = cx.empty_object().upcast();
            neon_serde::from_value(&mut cx, obj)?
        }
    };

    let this = cx.this();
    let output = {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        let fm = c.cm.new_source_file(
            if options.filename.is_empty() {
                FileName::Anon
            } else {
                FileName::Real(options.filename.clone().into())
            },
            source.value(),
        );

        c.process_js_file(fm, options)
            .expect("failed to process js file")
    };

    Ok(neon_serde::to_value(&mut cx, &output)?)
}

fn transform_file(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let path = cx.argument::<JsString>(0)?;
    let path_value = path.value();
    let path = Path::new(&path_value);

    let options_arg = cx.argument::<JsValue>(1)?;
    let options: Options = neon_serde::from_value(&mut cx, options_arg)?;
    let callback = cx.argument::<JsFunction>(2)?;

    let this = cx.this();
    {
        let guard = cx.lock();
        let c = this.borrow(&guard);

        TransformFileTask {
            c: c.clone(),
            path: path.into(),
            options,
        }
        .schedule(callback);
    };

    Ok(cx.undefined().upcast())
}

fn transform_file_sync(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let path = cx.argument::<JsString>(0)?;
    let opts: Options = match cx.argument_opt(1) {
        Some(v) => neon_serde::from_value(&mut cx, v)?,
        None => {
            let obj = cx.empty_object().upcast();
            neon_serde::from_value(&mut cx, obj)?
        }
    };

    let path_value = path.value();
    let path_value = clean(&path_value);
    let path = Path::new(&path_value);

    let this = cx.this();
    let output = {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        let fm = c.cm.load_file(path).expect("failed to load file");
        c.process_js_file(fm, opts)
            .expect("failed to process js file")
    };

    Ok(neon_serde::to_value(&mut cx, &output)?)
}

// ----- Parsing -----

struct ParseTask {
    c: Arc<Compiler>,
    fm: Arc<SourceFile>,
    options: ParseOptions,
}

struct ParseFileTask {
    c: Arc<Compiler>,
    path: PathBuf,
    options: ParseOptions,
}

fn complete_parse<'a>(
    mut cx: TaskContext<'a>,
    result: Result<Module, Error>,
    c: &Compiler,
) -> JsResult<'a, JsValue> {
    c.run(|| {
        common::CM.set(&c.cm, || match result {
            Ok(module) => Ok(neon_serde::to_value(&mut cx, &module)?),
            Err(err) => cx.throw_error(err.to_string()),
        })
    })
}

impl Task for ParseTask {
    type Output = Module;
    type Error = Error;
    type JsEvent = JsValue;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        let comments = Default::default();

        self.c.parse_js(
            self.fm.clone(),
            self.options.syntax,
            if self.options.comments {
                Some(&comments)
            } else {
                None
            },
        )
    }

    fn complete(
        self,
        cx: TaskContext,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        complete_parse(cx, result, &self.c)
    }
}

impl Task for ParseFileTask {
    type Output = Module;
    type Error = Error;
    type JsEvent = JsValue;

    fn perform(&self) -> Result<Self::Output, Self::Error> {
        let comments = Default::default();
        let fm = self
            .c
            .cm
            .load_file(&self.path)
            .map_err(|err| Error::FailedToReadModule { err })?;

        self.c.parse_js(
            fm,
            self.options.syntax,
            if self.options.comments {
                Some(&comments)
            } else {
                None
            },
        )
    }

    fn complete(
        self,
        cx: TaskContext,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        complete_parse(cx, result, &self.c)
    }
}

fn parse(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let src = cx.argument::<JsString>(0)?;
    let options_arg = cx.argument::<JsValue>(1)?;
    let options: ParseOptions = neon_serde::from_value(&mut cx, options_arg)?;
    let callback = cx.argument::<JsFunction>(2)?;

    let this = cx.this();
    {
        let guard = cx.lock();
        let c = this.borrow(&guard);

        let fm = c.cm.new_source_file(FileName::Anon, src.value());

        ParseTask {
            c: c.clone(),
            fm,
            options,
        }
        .schedule(callback);
    };

    Ok(cx.undefined().upcast())
}

fn parse_sync(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let src = cx.argument::<JsString>(0)?;
    let options_arg = cx.argument::<JsValue>(1)?;
    let options: ParseOptions = neon_serde::from_value(&mut cx, options_arg)?;

    let cm;
    let compiler;
    let this = cx.this();
    let module = {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        cm = c.cm.clone();
        compiler = c.clone();
        let fm = c.cm.new_source_file(FileName::Anon, src.value());
        let comments = Default::default();
        c.parse_js(
            fm,
            options.syntax,
            if options.comments {
                Some(&comments)
            } else {
                None
            },
        )
    };
    let module = match module {
        Ok(v) => v,
        Err(err) => return cx.throw_error(err.to_string()),
    };

    let v = compiler.run(|| common::CM.set(&cm, || Ok(neon_serde::to_value(&mut cx, &module)?)));
    v
}

fn parse_file_sync(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let path = cx.argument::<JsString>(0)?;
    let options_arg = cx.argument::<JsValue>(1)?;
    let options: ParseOptions = neon_serde::from_value(&mut cx, options_arg)?;

    let this = cx.this();
    let compiler;
    let module = {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        compiler = c.clone();
        let fm =
            c.cm.load_file(Path::new(&path.value()))
                .expect("failed to read module file");
        let comments = Default::default();

        c.parse_js(
            fm,
            options.syntax,
            if options.comments {
                Some(&comments)
            } else {
                None
            },
        )
    };
    let module = match module {
        Ok(v) => v,
        Err(err) => return cx.throw_error(err.to_string()),
    };

    Ok(compiler.run(|| neon_serde::to_value(&mut cx, &module))?)
}

fn parse_file(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let path = cx.argument::<JsString>(0)?;
    let options_arg = cx.argument::<JsValue>(1)?;
    let options: ParseOptions = neon_serde::from_value(&mut cx, options_arg)?;
    let callback = cx.argument::<JsFunction>(2)?;

    let this = cx.this();
    {
        let guard = cx.lock();
        let c = this.borrow(&guard);

        ParseFileTask {
            c: c.clone(),
            path: path.value().into(),
            options,
        }
        .schedule(callback);
    };

    Ok(cx.undefined().upcast())
}

// ----- Printing -----

struct PrintTask {
    c: Arc<Compiler>,
    module: Module,
    options: Options,
}

impl Task for PrintTask {
    type Output = TransformOutput;
    type Error = Error;
    type JsEvent = JsValue;
    fn perform(&self) -> Result<Self::Output, Self::Error> {
        let loc = self.c.cm.lookup_char_pos(self.module.span().lo());
        let fm = loc.file;
        let comments = Default::default();

        self.c.print(
            &self.module,
            fm,
            &comments,
            self.options.source_maps.is_some(),
            self.options
                .config
                .clone()
                .unwrap_or_default()
                .minify
                .unwrap_or(false),
        )
    }

    fn complete(
        self,
        cx: TaskContext,
        result: Result<Self::Output, Self::Error>,
    ) -> JsResult<Self::JsEvent> {
        complete_output(cx, result)
    }
}

fn print(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let module = cx.argument::<JsValue>(0)?;
    let module: Module = neon_serde::from_value(&mut cx, module)?;

    let options = cx.argument::<JsValue>(1)?;
    let options: Options = neon_serde::from_value(&mut cx, options)?;

    let callback = cx.argument::<JsFunction>(2)?;

    let this = cx.this();
    {
        let guard = cx.lock();
        let c = this.borrow(&guard);

        PrintTask {
            c: c.clone(),
            module,
            options,
        }
        .schedule(callback)
    }

    Ok(cx.undefined().upcast())
}

fn print_sync(mut cx: MethodContext<JsCompiler>) -> JsResult<JsValue> {
    let module = cx.argument::<JsValue>(0)?;
    let module: Module = neon_serde::from_value(&mut cx, module)?;

    let options = cx.argument::<JsValue>(1)?;
    let options: Options = neon_serde::from_value(&mut cx, options)?;

    let this = cx.this();
    let result = {
        let guard = cx.lock();
        let c = this.borrow(&guard);
        let loc = c.cm.lookup_char_pos(module.span().lo());
        let fm = loc.file;
        let comments = Default::default();

        c.print(
            &module,
            fm,
            &comments,
            options.source_maps.is_some(),
            options.config.unwrap_or_default().minify.unwrap_or(false),
        )
    };
    let result = match result {
        Ok(v) => v,
        Err(err) => return cx.throw_error(err.to_string()),
    };

    Ok(neon_serde::to_value(&mut cx, &result)?)
}

pub type ArcCompiler = Arc<Compiler>;

declare_types! {
    pub class JsCompiler for ArcCompiler {
        init(cx) {
            init(cx)
        }

        method transform(cx) {
            transform(cx)
        }

        method transformSync(cx) {
            transform_sync(cx)
        }

        method transformFile(cx) {
            transform_file(cx)
        }

        method transformFileSync(cx) {
            transform_file_sync(cx)
        }

        method parse(cx) {
            parse(cx)
        }

        method parseSync(cx) {
            parse_sync(cx)
        }

        method parseFile(cx) {
            parse_file(cx)
        }

        method parseFileSync(cx) {
            parse_file_sync(cx)
        }

        method print(cx) {
            print(cx)
        }

        method printSync(cx) {
            print_sync(cx)
        }
    }
}

register_module!(mut cx, {
    cx.export_class::<JsCompiler>("Compiler")?;
    Ok(())
});
