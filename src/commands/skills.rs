use jade_symphony::skill_status::{
    build_skill_readiness_report, render_skill_readiness_report,
    render_skill_readiness_report_json, SkillStatusInput,
};

pub(crate) fn skills_status(
    input: SkillStatusInput,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = build_skill_readiness_report(input);
    if json {
        println!("{}", render_skill_readiness_report_json(&report)?);
    } else {
        println!("{}", render_skill_readiness_report(&report));
    }
    Ok(())
}
