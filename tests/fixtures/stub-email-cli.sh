#!/bin/sh
# Minimal stub of email-cli (v0.6+) for tests.
# Returns canned JSON for the subcommands mailing-list-cli actually calls.
# Strips the leading --json flag so the case statements work cleanly.

if [ "$1" = "--json" ]; then
    shift
fi

case "$1" in
    "agent-info")
        echo '{"name":"email-cli","version":"0.6.2","commands":{}}'
        exit 0
        ;;
    "profile")
        if [ "$2" = "test" ]; then
            echo '{"version":"1","status":"success","data":{"reachable":true}}'
            exit 0
        fi
        ;;
    "segment")
        case "$2" in
            "create")
                echo '{"version":"1","status":"success","data":{"id":"seg_test_12345","name":"stub"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"object":"list","data":[]}}'
                exit 0
                ;;
            "contact-add"|"contact-remove")
                echo '{"version":"1","status":"success","data":{"id":"seg_test_12345"}}'
                exit 0
                ;;
        esac
        ;;
    "contact")
        case "$2" in
            "create")
                # If MLC_STUB_CONTACT_DUPLICATE is set, simulate a duplicate
                if [ -n "$MLC_STUB_CONTACT_DUPLICATE" ]; then
                    echo "contact already exists" >&2
                    exit 1
                fi
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890"}}'
                exit 0
                ;;
            "list")
                echo '{"version":"1","status":"success","data":{"object":"list","data":[]}}'
                exit 0
                ;;
            "get"|"show")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890","email":"stub@example.com"}}'
                exit 0
                ;;
            "update")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890"}}'
                exit 0
                ;;
            "delete"|"rm")
                echo '{"version":"1","status":"success","data":{"id":"contact_test_67890","deleted":true}}'
                exit 0
                ;;
        esac
        ;;
    "email")
        if [ "$2" = "list" ] || [ "$2" = "ls" ]; then
            echo '{"version":"1","status":"success","data":{"object":"list","has_more":false,"data":[]}}'
            exit 0
        fi
        ;;
    "broadcast")
        case "$2" in
            "create"|"new")
                echo '{"version":"1","status":"success","data":{"id":"bc_test_abc"}}'
                exit 0
                ;;
            "send")
                echo '{"version":"1","status":"success","data":{"id":"bc_test_abc"}}'
                exit 0
                ;;
            "list"|"ls")
                echo '{"version":"1","status":"success","data":{"object":"list","data":[]}}'
                exit 0
                ;;
        esac
        ;;
esac

echo '{"version":"1","status":"error","error":{"code":"unsupported","message":"stub","suggestion":"this is a test stub"}}' >&2
exit 1
