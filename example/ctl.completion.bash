_drbd-reactorctl() {
    local i cur prev opts cmds
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    cmd=""
    opts=""

    for i in ${COMP_WORDS[@]}
    do
        case "${i}" in
            drbd-reactorctl)
                cmd="drbd-reactorctl"
                ;;
            
            cat)
                cmd+="__cat"
                ;;
            disable)
                cmd+="__disable"
                ;;
            edit)
                cmd+="__edit"
                ;;
            enable)
                cmd+="__enable"
                ;;
            evict)
                cmd+="__evict"
                ;;
            generate-completion)
                cmd+="__generate__completion"
                ;;
            help)
                cmd+="__help"
                ;;
            ls)
                cmd+="__ls"
                ;;
            restart)
                cmd+="__restart"
                ;;
            rm)
                cmd+="__rm"
                ;;
            start-until)
                cmd+="__start__until"
                ;;
            status)
                cmd+="__status"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        drbd-reactorctl)
            opts=" -h -V -c  --local --help --version --config --context --nodes   disable enable status restart edit rm evict cat ls start-until generate-completion help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                    -c)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        
        drbd__reactorctl__cat)
            opts=" -h -V  --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__disable)
            opts=" -h -V  --now --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__edit)
            opts=" -f -h -V -t  --force --disabled --help --version --type --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --type)
                    COMPREPLY=($(compgen -W "promoter prometheus agentx umh debugger" -- "${cur}"))
                    return 0
                    ;;
                    -t)
                    COMPREPLY=($(compgen -W "promoter prometheus agentx umh debugger" -- "${cur}"))
                    return 0
                    ;;
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__enable)
            opts=" -h -V  --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__evict)
            opts=" -f -k -u -h -V -d  --force --keep-masked --unmask --help --version --delay --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --delay)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                    -d)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__generate__completion)
            opts=" -h -V  --help --version --context --nodes  <shell> "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__help)
            opts=" -h -V  --help --version --context --nodes  "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__ls)
            opts=" -h -V  --disabled --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__restart)
            opts=" -h -V  --with-targets --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__rm)
            opts=" -f -h -V  --disabled --force --help --version --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__start__until)
            opts=" -h -V  --help --version --context --nodes  <until> <configs> "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        drbd__reactorctl__status)
            opts=" -v -h -V -r  --verbose --json --help --version --resource --context --nodes  <configs>... "
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                
                --resource)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                    -r)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --context)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --nodes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

complete -F _drbd-reactorctl -o bashdefault -o default drbd-reactorctl
