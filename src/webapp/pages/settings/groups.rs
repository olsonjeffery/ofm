use leptos::prelude::*;

use crate::webapp::components::settings_sidebar::{
    SettingsSection, SettingsSidebar, SettingsSubPage,
};

pub fn render(_active: SettingsSubPage) -> String {
    let sidebar = leptos::view! {
        <SettingsSidebar section=SettingsSection::Admin active=SettingsSubPage::Groups />
    }
    .to_html();

    let pane = leptos::view! {
        <div class="box">
            <h3 class="title is-4">
                <span class="icon is-medium"><i class="mdi mdi-account-group"></i></span>
                "Groups & Organizations"
            </h3>
            <p class="mb-3">
                "Groups (user groups) and Organizations gate access to shared resources: a project,
                model configuration or task flow owned by a group member is readable by the group's
                other members and editable by contributors and above."
            </p>
            <div id="groups-list">
                <p class="has-text-grey">"Loading groups..."</p>
            </div>
            <div id="group-detail" style="display:none"></div>
        </div>

        <div class="box">
            <h3 class="title is-4">
                <span class="icon is-medium"><i class="mdi mdi-plus-circle-outline"></i></span>
                "Create Group"
            </h3>
            <div class="field">
                <label class="label">"Name"</label>
                <div class="control">
                    <input class="input" type="text" id="new-group-name" placeholder="e.g. engineering"/>
                </div>
            </div>
            <div class="field">
                <label class="label">"Type"</label>
                <div class="control">
                    <div class="select">
                        <select id="new-group-type">
                            <option value="user-group">"User Group"</option>
                            <option value="org">"Organization"</option>
                        </select>
                    </div>
                </div>
                <p class="help">"This choice is permanent and cannot be changed after creation."</p>
            </div>
            <div class="field">
                <label class="checkbox">
                    <input type="checkbox" id="new-group-oauth-scope"/>
                    " Create group whose name matches an OAuth scope (membership derived from granted scopes)"
                </label>
            </div>
            <div class="field" id="new-group-scope-field" style="display:none">
                <label class="label">"Scope"</label>
                <div class="control">
                    <div class="select">
                        <select id="new-group-scope-select">
                            <option value="">"— choose an advertised scope —"</option>
                        </select>
                    </div>
                    <input class="input mt-2" type="text" id="new-group-scope-custom" placeholder="or type a custom scope name"/>
                </div>
                <p class="help">"Users whose captured OAuth scopes include this name are members (read-only)."</p>
            </div>
            <div class="field">
                <label class="label">"Title"</label>
                <div class="control">
                    <input class="input" type="text" id="new-group-title" placeholder="Display title"/>
                </div>
            </div>
            <div class="field">
                <label class="label">"Description"</label>
                <div class="control">
                    <textarea class="textarea" id="new-group-description" placeholder="What is this group for?"></textarea>
                </div>
            </div>
            <button class="button is-primary" id="btn-create-group">
                <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                <span>"Create Group"</span>
            </button>
        </div>
    }
    .to_html();

    format!(
        r#"<section class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-account-group"></i></span>
                "Admin"
            </h2>
            <div class="columns">
                <div class="column is-one-quarter">
                    {}
                </div>
                <div class="column">
                    {}
                </div>
            </div>
        </section>
        <script>{}</script>"#,
        sidebar, pane, GROUPS_JS
    )
}

const GROUPS_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

document.addEventListener('DOMContentLoaded', function() {
    loadGroups();
    loadScopes();
    loadUsers();

    var createBtn = document.getElementById('btn-create-group');
    if (createBtn) createBtn.addEventListener('click', createGroup);

    var scopeToggle = document.getElementById('new-group-oauth-scope');
    if (scopeToggle) {
        scopeToggle.addEventListener('change', function() {
            var on = scopeToggle.checked;
            document.getElementById('new-group-scope-field').style.display = on ? 'block' : 'none';
        });
    }
});

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str == null ? '' : String(str)));
    return div.innerHTML;
}

function loadScopes() {
    apiCall('/api/groups/scopes-available')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            var sel = document.getElementById('new-group-scope-select');
            if (!sel) return;
            (data.scopes || []).forEach(function(s) {
                var opt = document.createElement('option');
                opt.value = s;
                opt.textContent = s;
                sel.appendChild(opt);
            });
        })
        .catch(function() {});
}

function loadUsers() {
    apiCall('/api/admin/users')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            var memberSel = document.getElementById('member-user-select');
            if (memberSel) {
                (data.users || []).forEach(function(u) {
                    var opt = document.createElement('option');
                    opt.value = u.id;
                    opt.textContent = u.username;
                    memberSel.appendChild(opt);
                });
            }
            var ownerSel = document.getElementById('owner-user-select');
            if (ownerSel) {
                (data.users || []).forEach(function(u) {
                    var opt = document.createElement('option');
                    opt.value = u.id;
                    opt.textContent = u.username;
                    ownerSel.appendChild(opt);
                });
            }
        })
        .catch(function() {});
}

function loadGroupOptions(currentGroupId) {
    apiCall('/api/groups')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            var sel = document.getElementById('member-group-select');
            if (!sel) return;
            (data.groups || []).forEach(function(g) {
                if (g.id === currentGroupId) return;
                var opt = document.createElement('option');
                opt.value = g.id;
                opt.textContent = g.name;
                sel.appendChild(opt);
            });
        })
        .catch(function() {});
}

function loadGroups() {
    var list = document.getElementById('groups-list');
    apiCall('/api/groups')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (!data.groups || data.groups.length === 0) {
                list.innerHTML = '<p>No groups yet. Create one below.</p>';
                return;
            }
            var html = '<table class="table is-fullwidth is-hoverable"><thead><tr>' +
                '<th>Name</th><th>Type</th><th>Owner</th><th>Members</th><th>Scope</th><th></th></tr></thead><tbody>';
            data.groups.forEach(function(g) {
                var type = g.is_org ? '<span class="tag">Org</span>' : '<span class="tag is-info">User Group</span>';
                var admins = g.is_admins_group ? ' <span class="tag is-warning">admins</span>' : '';
                var scope = g.is_oauth_scope ? '<span class="tag is-success">oauth-scope</span>' : '';
                html += '<tr>';
                html += '<td><strong>' + escapeHtml(g.title || g.name) + '</strong>' + admins + '<br/><span class="has-text-grey">' + escapeHtml(g.name) + '</span></td>';
                html += '<td>' + type + '</td>';
                html += '<td>' + escapeHtml(g.owner_username) + '</td>';
                html += '<td>' + g.member_count + '</td>';
                html += '<td>' + scope + '</td>';
                html += '<td><button class="button is-small" onclick="showGroup(\'' + g.id + '\')"><span class="icon is-small"><i class="mdi mdi-account-edit"></i></span><span>Manage</span></button> ';
                if (!g.is_admins_group) {
                    html += '<button class="button is-small is-danger" onclick="deleteGroup(\'' + g.id + '\', \'' + escapeHtml(g.name) + '\')"><span class="icon is-small"><i class="mdi mdi-delete"></i></span><span>Delete</span></button>';
                }
                html += '</td></tr>';
            });
            html += '</tbody></table>';
            list.innerHTML = html;
        })
        .catch(function(err) {
            list.innerHTML = '<p class="has-text-danger">Failed to load groups: ' + err + '</p>';
        });
}

function createGroup() {
    var name = document.getElementById('new-group-name').value.trim();
    if (!name) { alert('Name is required.'); return; }
    var type = document.getElementById('new-group-type').value;
    var isOrg = type === 'org';
    var isOauthScope = document.getElementById('new-group-oauth-scope').checked;
    var scopeName = name;
    if (isOauthScope) {
        var sel = document.getElementById('new-group-scope-select').value;
        var custom = document.getElementById('new-group-scope-custom').value.trim();
        scopeName = custom || sel || name;
    }
    apiCall('/api/groups', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            name: isOauthScope ? scopeName : name,
            is_org: isOrg,
            is_oauth_scope: isOauthScope,
            title: document.getElementById('new-group-title').value.trim(),
            description: document.getElementById('new-group-description').value.trim()
        })
    })
    .then(function(r) {
        if (!r.ok) { return r.json().then(function(d) { throw new Error(d.error || 'Create failed'); }); }
        return r.json();
    })
    .then(function() {
        document.getElementById('new-group-name').value = '';
        document.getElementById('new-group-scope-custom').value = '';
        document.getElementById('new-group-title').value = '';
        document.getElementById('new-group-description').value = '';
        loadGroups();
    })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function showGroup(id) {
    apiCall('/api/groups/' + id)
        .then(function(r) { return r.json(); })
        .then(function(g) {
            renderGroupDetail(g);
        })
        .catch(function(err) { alert('Error loading group: ' + err.message); });
}

function renderGroupDetail(g) {
    var detail = document.getElementById('group-detail');
    var isAdminGroup = g.is_admins_group;
    detail.style.display = 'block';
    detail.innerHTML =
        '<h4 class="title is-5">Manage: ' + escapeHtml(g.title || g.name) + ' (' + escapeHtml(g.name) + ')</h4>' +
        '<div class="field"><label class="label">Title</label><div class="control"><input class="input" id="edit-group-title" value="' + escapeHtml(g.title) + '"/></div></div>' +
        '<div class="field"><label class="label">Description</label><div class="control"><textarea class="textarea" id="edit-group-description">' + escapeHtml(g.description) + '</textarea></div></div>' +
        '<button class="button is-small" onclick="saveGroupMeta(\'' + g.id + '\')"><span class="icon is-small"><i class="mdi mdi-content-save"></i></span><span>Save</span></button>' +
        (g.is_org ? '<span class="tag ml-2">Organization</span>' : '<span class="tag is-info ml-2">User Group</span>') +
        '<span class="tag ml-2">Owner: ' + escapeHtml(g.owner_username) + '</span>' +
        '<div class="field has-addons mt-2"><label class="label mr-2">Change owner (admin only)</label>' +
            '<div class="control"><div class="select"><select id="owner-user-select"></select></div></div>' +
            '<div class="control"><button class="button is-small is-primary" onclick="changeOwner(\'' + g.id + '\')">Set Owner</button></div>' +
        '</div>' +
        '<hr/>' +
        '<h5 class="title is-6">Members</h5>' +
        '<div class="field has-addons">' +
            '<div class="control"><div class="select"><select id="member-user-select"></select></div></div>' +
            '<div class="control"><div class="select"><select id="member-level-select"><option value="read-only">read-only</option><option value="contributor">contributor</option><option value="maintainer">maintainer</option><option value="admin">admin</option></select></div></div>' +
            '<div class="control"><button class="button is-small is-primary" onclick="addUserMember(\'' + g.id + '\')">Add</button></div>' +
        '</div>' +
        '<div class="field has-addons">' +
            '<div class="control"><div class="select"><select id="member-group-select"></select></div></div>' +
            '<div class="control"><div class="select"><select id="member-group-level-select"><option value="read-only">read-only</option><option value="contributor">contributor</option><option value="maintainer">maintainer</option><option value="admin">admin</option></select></div></div>' +
            '<div class="control"><button class="button is-small is-primary" onclick="addSubgroupMember(\'' + g.id + '\')">Add Subgroup</button></div>' +
        '</div>' +
        '<div id="member-list"><p class="has-text-grey">Loading members...</p></div>';

    loadUsers();
    loadGroupOptions(g.id);
    loadMembers(g.id);
}

function changeOwner(groupId) {
    var ownerSel = document.getElementById('owner-user-select');
    if (!ownerSel || !ownerSel.value) { alert('Choose a user to set as owner.'); return; }
    apiCall('/api/groups/' + groupId, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner_id: ownerSel.value })
    })
    .then(function(r) {
        if (!r.ok) { return r.json().then(function(d) { throw new Error(d.error || 'Owner change failed'); }); }
        return r.json();
    })
    .then(function() { showGroup(groupId); loadGroups(); })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function addSubgroupMember(groupId) {
    var groupSel = document.getElementById('member-group-select');
    var levelSel = document.getElementById('member-group-level-select');
    if (!groupSel || !groupSel.value) { alert('Choose a subgroup to add.'); return; }
    apiCall('/api/groups/' + groupId + '/members', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ member_group_id: groupSel.value, level: levelSel.value })
    })
    .then(function(r) {
        if (!r.ok) { return r.json().then(function(d) { throw new Error(d.error || 'Add failed'); }); }
        return r.json();
    })
    .then(function() { loadMembers(groupId); loadGroups(); })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function saveGroupMeta(id) {
    apiCall('/api/groups/' + id, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            title: document.getElementById('edit-group-title').value.trim(),
            description: document.getElementById('edit-group-description').value.trim()
        })
    })
    .then(function(r) {
        if (!r.ok) throw new Error('Save failed');
        loadGroups();
    })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function loadMembers(groupId) {
    apiCall('/api/groups/' + groupId + '/members')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            var list = document.getElementById('member-list');
            if (!data.members || data.members.length === 0) {
                list.innerHTML = '<p class="has-text-grey">No members yet.</p>';
                return;
            }
            var html = '<table class="table is-fullwidth"><thead><tr><th>Member</th><th>Level</th><th></th></tr></thead><tbody>';
            data.members.forEach(function(m) {
                var label = m.username ? escapeHtml(m.username) + ' <span class="tag">user</span>' : escapeHtml(m.subgroup_name) + ' <span class="tag is-info">group</span>';
                html += '<tr><td>' + label + '</td><td>' +
                    '<div class="select"><select onchange="changeLevel(\'' + groupId + '\', \'' + m.id + '\', this.value)">' +
                    '<option value="read-only"' + (m.level === 'read-only' ? ' selected' : '') + '>read-only</option>' +
                    '<option value="contributor"' + (m.level === 'contributor' ? ' selected' : '') + '>contributor</option>' +
                    '<option value="maintainer"' + (m.level === 'maintainer' ? ' selected' : '') + '>maintainer</option>' +
                    '<option value="admin"' + (m.level === 'admin' ? ' selected' : '') + '>admin</option>' +
                    '</select></div></td>' +
                    '<td><button class="button is-small is-danger" onclick="removeMember(\'' + groupId + '\', \'' + m.id + '\')"><span class="icon is-small"><i class="mdi mdi-close"></i></span></button></td></tr>';
            });
            html += '</tbody></table>';
            list.innerHTML = html;
        })
        .catch(function(err) { alert('Error loading members: ' + err.message); });
}

function addUserMember(groupId) {
    var userSel = document.getElementById('member-user-select');
    var levelSel = document.getElementById('member-level-select');
    if (!userSel || !userSel.value) { alert('Choose a user to add.'); return; }
    apiCall('/api/groups/' + groupId + '/members', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ user_id: userSel.value, level: levelSel.value })
    })
    .then(function(r) {
        if (!r.ok) { return r.json().then(function(d) { throw new Error(d.error || 'Add failed'); }); }
        return r.json();
    })
    .then(function() { loadMembers(groupId); loadGroups(); })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function changeLevel(groupId, memberId, level) {
    apiCall('/api/groups/' + groupId + '/members/' + memberId, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ level: level })
    })
    .then(function(r) {
        if (!r.ok) throw new Error('Level change failed');
        loadMembers(groupId);
    })
    .catch(function(err) { alert('Error: ' + err.message); });
}

function removeMember(groupId, memberId) {
    apiCall('/api/groups/' + groupId + '/members/' + memberId, { method: 'DELETE' })
        .then(function(r) {
            if (!r.ok) throw new Error('Remove failed');
            loadMembers(groupId);
            loadGroups();
        })
        .catch(function(err) { alert('Error: ' + err.message); });
}

function deleteGroup(id, name) {
    if (!confirm('Delete group "' + name + '"? Memberships are removed with it.')) return;
    apiCall('/api/groups/' + id, { method: 'DELETE' })
        .then(function(r) {
            if (!r.ok) throw new Error('Delete failed');
            document.getElementById('group-detail').style.display = 'none';
            loadGroups();
        })
        .catch(function(err) { alert('Error: ' + err.message); });
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_groups_page_renders() {
        let html = render(SettingsSubPage::Groups);
        assert!(html.contains("menu"));
        assert!(html.contains("Groups &amp; Organizations"));
        assert!(html.contains("btn-create-group"));
        assert!(html.contains("new-group-oauth-scope"));
        assert!(html.contains("new-group-type"));
        assert!(html.contains("permanent"));
        assert!(html.contains("/api/groups/scopes-available"));
        assert!(html.contains("addSubgroupMember"));
        assert!(html.contains("changeOwner"));
        assert!(html.contains("member-group-select"));
        assert!(html.contains("owner-user-select"));
    }

    #[test]
    fn test_groups_page_sidebar_admin() {
        let html = render(SettingsSubPage::Groups);
        assert!(html.contains("/webapp/settings/admin/groups"));
        assert!(html.contains("is-active"));
    }
}
