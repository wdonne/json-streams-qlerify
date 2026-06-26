use crate::{
    ApiArgs,
    common::{get_file, read_json, read_json_or_yaml, write_yaml},
    error::ToolError,
    model::{Aggregate, App, Field, Related, SchemaObject, extract},
};
use generic_builders::immutable::Builder;
use immutable_json::api::Value;
use immutable_json::array::Array;
use immutable_json::object::Object;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

const OPENAPI_VERSION: &str = "3.2.0";

fn accepted() -> Object {
    Object::new().add_string("description", "Accepted")
}

fn add_aggregate_properties(entity_schemas: &Object, app: &App) -> Object {
    entity_schemas
        .iter()
        .filter_map(|(k, v)| {
            v.as_object()
                .and_then(|o| app.aggregates.get(&k).map(|a| (k, o, a)))
        })
        .fold(entity_schemas.clone(), |s, (k, e, a)| {
            s.add_object(
                &k,
                &add_technical_properties(&e, &type_name(app, a), None, &["_seq"]),
            )
        })
}

fn add_technical_properties(
    schema: &Object,
    type_name: &str,
    extra_schema: Option<Object>,
    extra_required: &[&str],
) -> Object {
    schema
        .add_object(
            "properties",
            &schema
                .get_object("properties")
                .unwrap_or_default()
                .merge(&technical_fields_schema(type_name))
                .merge(&seq_schema())
                .merge(&jwt_schema())
                .merge(&extra_schema.unwrap_or_default()),
        )
        .add_array(
            "required",
            &add_technical_required(
                &schema.get_array("required").unwrap_or_default(),
                extra_required,
            ),
        )
}

fn add_technical_required(array: &Array, extra_required: &[&str]) -> Array {
    extra_required.iter().fold(
        array
            .add_string("_id")
            .add_string("_corr")
            .add_string("_type"),
        |a, r| a.add_string(r),
    )
}

fn aggregate_commands_schema(app: &App) -> Object {
    app.aggregates.values().fold(Object::new(), |o, a| {
        o.add_object(&("Commands".to_string() + &a.entity), &commands_schema(a))
    })
}

fn aggregate_ref(aggregate: &Aggregate) -> String {
    entity_ref(&aggregate.entity)
}

fn aggregate_refs(app: &App) -> Array {
    app.aggregates.values().fold(Array::new(), |arr, a| {
        arr.add_object(&ref_object(&aggregate_ref(a)))
    })
}

fn any_object() -> Object {
    Object::new().add_object(
        "schema",
        &Object::new()
            .add_string("type", "object")
            .add_object("additionalProperties", &Object::new()),
    )
}

fn boolean_type() -> Object {
    Object::new().add_string("type", "boolean")
}

fn command(aggregate: &Aggregate) -> Object {
    Object::new()
        .add_string(
            "summary",
            &("Post command to ".to_string() + &aggregate.entity),
        )
        .add_array("parameters", &Array::new().add_object(&id_parameter()))
        .add_object(
            "requestBody",
            &Object::new().add_object(
                "content",
                &data_content(&("#/components/schemas/Commands".to_string() + &aggregate.entity)),
            ),
        )
        .add_object(
            "responses",
            &Object::new()
                .add_object("202", &accepted())
                .add_object("401", &unauthorized()),
        )
}

fn command_error_schema() -> Object {
    Object::new()
        .add_object("_error", &boolean_type())
        .add_object(
            "errors",
            &Object::new().add_string("type", "array").add_object(
                "items",
                &Object::new().add_string("type", "object").add_object(
                    "properties",
                    &Object::new()
                        .add_object("location", &string_type())
                        .add_object("code", &string_type()),
                ),
            ),
        )
}

fn command_field_schema(command: &SchemaObject) -> Object {
    Object::new().add_object("_command", &const_type(&command.name))
}

fn command_name(aggregate: &Aggregate, command: &SchemaObject) -> String {
    "Command".to_string() + &aggregate.entity + &command.name
}

fn command_ref(aggregate: &Aggregate, command: &SchemaObject) -> String {
    "#/components/schemas/".to_string() + &command_name(aggregate, command)
}

fn command_refs(app: &App) -> Array {
    flatten_commands(app).fold(Array::new(), |arr, (a, c)| {
        arr.add_object(&ref_object(&command_ref(a, c)))
    })
}

fn command_schema(command: &SchemaObject, app: &App, aggregate: &Aggregate) -> Object {
    add_technical_properties(
        &schema_object_schema(command),
        &type_name(app, aggregate),
        Some(command_field_schema(command)),
        &["_command"],
    )
}

fn command_schemas(app: &App, with_errors: bool) -> Object {
    flatten_commands(app).fold(Object::new(), |s, (a, c)| {
        s.add_object(
            &command_name(a, c),
            &Builder::new(command_schema(c, app, a))
                .update_if(
                    |_| with_errors,
                    |o| {
                        o.add_object(
                            "properties",
                            &o.get_object("properties")
                                .unwrap_or_default()
                                .merge(&command_error_schema()),
                        )
                    },
                )
                .build(),
        )
    })
}

fn commands_schema(aggregate: &Aggregate) -> Object {
    Object::new().add_array(
        "anyOf",
        &aggregate.commands.values().fold(Array::new(), |a, c| {
            a.add_object(&ref_object(&command_ref(aggregate, c)))
        }),
    )
}

fn common_schema(app: &App, with_errors: bool) -> Object {
    aggregate_commands_schema(app)
        .merge(&command_schemas(app, with_errors))
        .merge(&add_aggregate_properties(
            &schema_object_schemas(app.entities.values().cloned(), "Entity"),
            app,
        ))
        .merge(&schema_object_schemas(
            app.value_objects.values().cloned(),
            "ValueObject",
        ))
}

fn const_type(value: &str) -> Object {
    Object::new().add_string("const", value)
}

fn create_aggregate_api(app: &App, directory: &Path, template: &Object) -> Result<(), ToolError> {
    let real_template = merge_info(template, app);

    create_aggregate_api_http(app, &mut get_file(directory, "http.yaml")?, &real_template)?;
    create_aggregate_api_sse(app, &mut get_file(directory, "sse.yaml")?, &real_template)?;

    Ok(())
}

fn create_aggregate_api_http(
    app: &App,
    file: &mut File,
    template: &Object,
) -> Result<(), ToolError> {
    write_yaml(
        &Value::Object(template.add_object("paths", &http_paths(app)).add_object(
            "components",
            &Object::new().add_object("schemas", &common_schema(app, false)),
        )),
        file,
    )?;

    Ok(())
}

fn create_aggregate_api_sse(
    app: &App,
    file: &mut File,
    template: &Object,
) -> Result<(), ToolError> {
    write_yaml(
        &Value::Object(template.add_object("paths", &sse_paths()).add_object(
            "components",
            &Object::new().add_object("schemas", &common_schema(app, true).merge(&sse_schema(app))),
        )),
        file,
    )?;

    Ok(())
}

fn data_content(schema_ref: &str) -> Object {
    Object::new().add_object(
        "application/json",
        &Object::new().add_object("schema", &Object::new().add_string("$ref", schema_ref)),
    )
}

fn entity_ref(entity: &str) -> String {
    "#/components/schemas/Entity".to_string() + entity
}

fn fetch_aggregate(aggregate: &Aggregate) -> Object {
    Object::new()
        .add_string("summary", &("Fetch ".to_string() + &aggregate.entity))
        .add_array("parameters", &Array::new().add_object(&id_parameter()))
        .add_object("responses", &fetch_responses(aggregate))
}

fn fetch_responses(aggregate: &Aggregate) -> Object {
    Object::new()
        .add_object(
            "200",
            &Object::new()
                .add_string("description", "The aggregate instance as a JSON object")
                .add_object("content", &data_content(&aggregate_ref(aggregate))),
        )
        .add_object("401", &unauthorized())
}

fn field_ref_schema(schema: &Object, data_type: &str, related: &Related) -> Object {
    let reference = match related {
        Related::Entity(e) => &entity_ref(e),
        Related::ValueObject(v) => &value_object_ref(v),
    };

    match data_type {
        "array" => schema
            .add_string("type", "array")
            .add_object("items", &ref_object(reference)),
        "object" => schema.add_string("$ref", reference),
        _ => schema.clone(),
    }
}

fn field_schema(field: &Field) -> Object {
    Builder::new(Object::new())
        .update_if(
            |_| field.related.is_none(),
            |o| o.add_string("type", &field.data_type),
        )
        .update_if_some(
            |_| field.related.as_ref(),
            |o, r| field_ref_schema(&o, &field.data_type, r),
        )
        .update_if_some(
            |_| field.description.as_ref(),
            |o, d| o.add_string("description", d),
        )
        .build()
}

fn fields_schema(iter: impl Iterator<Item = Field>) -> Object {
    iter.map(|f| (f.name.clone(), field_schema(&f)))
        .fold(Object::new(), |o, (n, s)| o.add_object(&n, &s))
}

fn flatten_commands(app: &App) -> impl Iterator<Item = (&Aggregate, &SchemaObject)> {
    app.aggregates
        .values()
        .flat_map(|a| a.commands.values().map(move |c| (a, c)))
}

pub(crate) fn generate(args: &ApiArgs) -> Result<(), ToolError> {
    create_aggregate_api(
        &extract(&read_json(&args.file)?)?,
        &args.directory,
        &read_template(args.template.as_ref())?.add_string("openapi", OPENAPI_VERSION),
    )
}

fn http_paths(app: &App) -> Object {
    app.aggregates
        .values()
        .fold(Object::new(), |o, a| http_paths_aggregate(app, &o, a))
}

fn http_paths_aggregate(app: &App, object: &Object, aggregate: &Aggregate) -> Object {
    let base_path =
        "/".to_string() + &app.name.to_lowercase() + "/" + &aggregate.entity.to_lowercase();

    object
        .add_object(&base_path, &search(aggregate))
        .add_object(
            &(base_path + "/{id}"),
            &Object::new()
                .add_object("get", &fetch_aggregate(aggregate))
                .add_object("post", &command(aggregate)),
        )
}

fn id_parameter() -> Object {
    Object::new()
        .add_string("name", "id")
        .add_string("in", "path")
        .add_bool("required", true)
        .add_object("schema", &uuid_type())
}

fn json_array_content() -> Object {
    Object::new().add_object(
        "application/json",
        &Object::new().add_object(
            "schema",
            &Object::new()
                .add_string("type", "array")
                .add_object("items", &any_object()),
        ),
    )
}

fn jwt_schema() -> Object {
    Object::new().add_object("_jwt", &any_object())
}

fn merge_info(template: &Object, app: &App) -> Object {
    template.add_object(
        "info",
        &template
            .get_object("info")
            .unwrap_or_default()
            .add_string("title", &app.name)
            .add_string("version", &app.version),
    )
}

fn positive_integer_type() -> Object {
    Object::new()
        .add_string("type", "integer")
        .add_integer("minimum", 0)
}

fn read_template(template: Option<&PathBuf>) -> Result<Object, ToolError> {
    if let Some(t) = template {
        Ok(read_json_or_yaml(t)?)
    } else {
        Ok(Object::new())
    }
}

fn ref_object(ref_name: &str) -> Object {
    Object::new().add_string("$ref", ref_name)
}

fn required_fields(iter: impl Iterator<Item = Field>) -> Array {
    iter.filter(|f| f.required)
        .map(|f| f.name)
        .fold(Array::new(), |a, n| a.add_string(&n))
}

fn schema_object_schema(object: &SchemaObject) -> Object {
    Builder::new(Object::new())
        .update(|o| o.add_string("type", "object"))
        .update(|o| {
            o.add_object(
                "properties",
                &fields_schema(object.fields.clone().into_iter()),
            )
        })
        .update(|o| {
            o.add_array(
                "required",
                &required_fields(object.fields.clone().into_iter()),
            )
        })
        .update_if_some(
            |_| object.description.as_ref(),
            |o, d| o.add_string("description", d),
        )
        .build()
}

fn schema_object_schemas(objects: impl Iterator<Item = SchemaObject>, prefix: &str) -> Object {
    objects.fold(Object::new(), |s, o| {
        s.add_object(&(prefix.to_string() + &o.name), &schema_object_schema(&o))
    })
}

fn search(aggregate: &Aggregate) -> Object {
    Object::new().add_object(
        "post",
        &Object::new()
            .add_string("summary", &("Search ".to_string() + &aggregate.entity))
            .add_string(
                "description",
                "Search aggregate instances with a MongoDB aggregation pipeline",
            )
            .add_object("requestBody", &search_request())
            .add_object("responses", &search_responses()),
    )
}

fn search_request() -> Object {
    Object::new()
        .add_string("description", "A MongoDB aggregation pipeline")
        .add_object("content", &json_array_content())
}

fn search_responses() -> Object {
    Object::new()
        .add_object(
            "200",
            &Object::new()
                .add_string("description", "The results in a JSON array")
                .add_object("content", &json_array_content()),
        )
        .add_object("401", &unauthorized())
}

fn seq_schema() -> Object {
    Object::new().add_object("_seq", &positive_integer_type())
}

fn sse_paths() -> Object {
    Object::new().add_object(
        "/",
        &Object::new().add_object(
            "get",
            &Object::new()
                .add_array("tags", &Array::new().add_string("ServerSentEvents"))
                .add_string(
                    "summary",
                    "The endpoint to receive a Server-Sent Event stream",
                )
                .add_object("responses", &sse_responses()),
        ),
    )
}

fn sse_responses() -> Object {
    Object::new()
        .add_object(
            "200",
            &Object::new()
                .add_string(
                    "description",
                    "The stream of aggregate instances and invalid commands",
                )
                .add_object(
                    "content",
                    &Object::new().add_object(
                        "text/event-stream",
                        &Object::new()
                            .add_object("schema", &string_type())
                            .add_object(
                                "itemSchema",
                                &Object::new().add_string("$ref", "#/components/schemas/Sse"),
                            ),
                    ),
                ),
        )
        .add_object("401", &unauthorized())
}

fn sse_schema(app: &App) -> Object {
    Object::new().add_object(
        "Sse",
        &Object::new().add_array("anyOf", &aggregate_refs(app).append(&command_refs(app))),
    )
}

fn string_type() -> Object {
    Object::new().add_string("type", "string")
}

fn technical_fields_schema(type_name: &str) -> Object {
    let uuid = uuid_type();

    Object::new()
        .add_object("_id", &uuid)
        .add_object("_corr", &uuid)
        .add_object("_type", &const_type(type_name))
}

fn type_name(app: &App, aggregate: &Aggregate) -> String {
    app.name.to_lowercase() + "-" + &aggregate.entity.to_lowercase()
}

fn unauthorized() -> Object {
    Object::new().add_string("description", "Unauthorized")
}

fn uuid_type() -> Object {
    Object::new()
        .add_string("type", "string")
        .add_string("format", "uuid")
}

fn value_object_ref(object: &str) -> String {
    "#/components/schemas/ValueObject".to_string() + object
}
