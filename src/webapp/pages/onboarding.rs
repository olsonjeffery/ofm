use leptos::prelude::*;

#[component]
pub fn OnboardingForm() -> impl IntoView {
    view! {
        <div class="onboarding-card">
            <h2>"Complete Your Profile"</h2>
            <p>"Set up your git identity to get started."</p>
            <form id="onboarding-form">
                <label>
                    "Git Name"
                    <input type="text" name="git_name" required placeholder="e.g. Jane Doe"/>
                </label>
                <label>
                    "Git Email"
                    <input type="email" name="git_email" required placeholder="e.g. jane@example.com"/>
                </label>
                <fieldset>
                    <legend>"Role"</legend>
                    <label>
                        <input type="radio" name="is_technical" value="true"/>
                        " Technical"
                    </label>
                    <label>
                        <input type="radio" name="is_technical" value="false" checked/>
                        " Non-technical"
                    </label>
                </fieldset>
                <button type="submit" class="btn btn-primary">"Complete Setup"</button>
            </form>
        </div>
    }
}

pub fn render_onboarding_form() -> String {
    crate::webapp::shim::render_component(|| view! { <OnboardingForm /> })
}
